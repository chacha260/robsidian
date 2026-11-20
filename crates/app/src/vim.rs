//! Vim風のモーダル編集エンジン。
//!
//! 本文（`String`）とカーソル（バイト位置）を受け取ってキーを1つずつ処理し、本文とカーソルを書き換える。
//! GUIに依存しないので、`|` でカーソルを書いたテキストでテストできる。
//! 挿入モード中の通常の文字入力はエディタ（TextEdit）に任せ、エンジンはEscだけを受け取る
//! （`.` の繰り返しで挿入を再生するときだけ、エンジン自身が文字を入れる）。

use crate::edit::indent_unit;
use std::collections::HashMap;

mod helix;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Insert,
    Visual,
    VisualLine,
    /// 矩形選択（Ctrl+V）
    VisualBlock,
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Mode::Normal => "NORMAL",
            Mode::Insert => "INSERT",
            Mode::Visual => "VISUAL",
            Mode::VisualLine => "V-LINE",
            Mode::VisualBlock => "V-BLOCK",
        }
    }

    pub fn is_visual(self) -> bool {
        matches!(self, Mode::Visual | Mode::VisualLine | Mode::VisualBlock)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Esc,
    Enter,
    Backspace,
    /// Ctrl+R（やり直し）
    CtrlR,
    /// Ctrl+D / Ctrl+U（半画面移動）
    CtrlD,
    CtrlU,
    /// Ctrl+O / Ctrl+I（Tab）: ジャンプリストを戻る / 進む
    CtrlO,
    CtrlI,
    /// Ctrl+V（Ctrl+Q）: 矩形選択
    CtrlV,
}

/// エディタ側で行ってほしいこと。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cmd {
    Save,
    Close,
    SaveClose,
    /// `/`（true）または `?`（false）で検索バーを開く
    OpenFind(bool),
    FindNext,
    FindPrev,
    /// `*` / `#`: カーソル下の語を検索する（trueなら前方）
    SearchWord(String, bool),
    Undo(usize),
    Redo(usize),
    /// `:noh`
    ClearSearch,
    /// 知らない `:` コマンド
    Unknown(String),
    /// 知らせること（置換で見つからなかったなど）
    Message(String),
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Effects {
    /// 本文を変えたか
    pub changed: bool,
    pub cmd: Option<Cmd>,
    /// ヤンク（コピー）した文字列。システムのクリップボードにも入れる
    pub yank: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Register {
    text: String,
    linewise: bool,
    /// 矩形選択でヤンクしたもの（行ごとに、同じ列に貼る）
    block: bool,
}

/// 矩形選択からの挿入（I / A / c）。Esc で、最初の行に入れた文字を残りの行にも入れる
#[derive(Debug, Clone, Copy)]
struct BlockInsert {
    /// 残りの行（行番号）
    lines: (usize, usize),
    /// 入れる列（文字数）
    col: usize,
    /// 行が短ければ空白で埋める（A）
    pad: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Exclusive,
    Inclusive,
    Linewise,
}

pub struct Vim {
    pub mode: Mode,
    /// ビジュアルモードの起点
    pub anchor: usize,
    keys: Vec<Key>,
    register: Register,
    /// 上下移動で保つ列（文字数）
    want_col: Option<usize>,
    last_find: Option<(char, bool, bool)>,
    last_change: Vec<Key>,
    /// 挿入モードに入ったコマンドのキーと、挿入を始めた位置・その時の本文の長さ
    insert_record: Option<(Vec<Key>, usize, usize)>,
    replaying: bool,
    /// `:` コマンドの入力中
    pub cmdline: Option<String>,
    /// Helix風（選択してから操作する）のキー操作
    pub helix: bool,
    /// エディタに最後に反映した選択（主, 副のバイト位置）と、そのときの頭（カーソル）の位置。
    /// エディタの選択がこのままなら頭の位置を使い、変わっていればマウスで動かしたとみなす
    pub synced: Option<(usize, usize, usize)>,
    /// 名前付きのレジスタ（a〜z）と、最後にヤンクしたもの（0）
    registers: HashMap<char, Register>,
    /// `"a` で指定した、次のコマンドが使うレジスタ
    pending_register: Option<char>,
    /// 記録中のマクロ（レジスタ, キー）
    recording: Option<(char, Vec<Key>)>,
    macros: HashMap<char, Vec<Key>>,
    last_macro: Option<char>,
    /// マクロの再生の深さ（再生中のキーは記録しない。再帰しすぎないように）
    macro_depth: usize,
    /// マーク（バイト位置）。`'` は最後のジャンプの前の位置、`<` `>` は最後のビジュアル選択
    marks: HashMap<char, usize>,
    /// ジャンプリスト（バイト位置）と、いまの位置（末尾なら jumps.len()）
    jumps: Vec<usize>,
    jump_at: usize,
    block_insert: Option<BlockInsert>,
}

impl Default for Vim {
    fn default() -> Self {
        Self {
            mode: Mode::Normal,
            anchor: 0,
            keys: Vec::new(),
            register: Register::default(),
            want_col: None,
            last_find: None,
            last_change: Vec::new(),
            insert_record: None,
            replaying: false,
            cmdline: None,
            helix: false,
            synced: None,
            registers: HashMap::new(),
            pending_register: None,
            recording: None,
            macros: HashMap::new(),
            last_macro: None,
            macro_depth: 0,
            marks: HashMap::new(),
            jumps: Vec::new(),
            jump_at: 0,
            block_insert: None,
        }
    }
}

// ---- 位置の計算（すべてバイト位置） ----

fn ch(t: &str, p: usize) -> Option<char> {
    t.get(p..)?.chars().next()
}
fn next(t: &str, p: usize) -> usize {
    ch(t, p).map_or(p, |c| p + c.len_utf8())
}
fn prev(t: &str, p: usize) -> usize {
    t[..p].chars().next_back().map_or(p, |c| p - c.len_utf8())
}
fn line_start(t: &str, p: usize) -> usize {
    t[..p].rfind('\n').map_or(0, |i| i + 1)
}
fn line_end(t: &str, p: usize) -> usize {
    t[p..].find('\n').map_or(t.len(), |i| p + i)
}
fn col(t: &str, p: usize) -> usize {
    t[line_start(t, p)..p].chars().count()
}
fn first_nonblank(t: &str, p: usize) -> usize {
    let s = line_start(t, p);
    let e = line_end(t, s);
    s + t[s..e].len() - t[s..e].trim_start_matches([' ', '\t']).len()
}
fn line_count(t: &str) -> usize {
    t.matches('\n').count() + 1
}
/// `n` 行目（0始まり）の先頭。
fn nth_line(t: &str, n: usize) -> usize {
    if n == 0 {
        return 0;
    }
    t.match_indices('\n')
        .nth(n - 1)
        .map_or_else(|| line_start(t, t.len()), |(i, _)| i + 1)
}
fn line_no(t: &str, p: usize) -> usize {
    t[..p].matches('\n').count()
}
/// 行内の `c` 列目（行が短ければ最後の文字）。
fn at_col(t: &str, ls: usize, c: usize) -> usize {
    let le = line_end(t, ls);
    let mut p = ls;
    for _ in 0..c {
        let n = next(t, p);
        if n >= le {
            break;
        }
        p = n;
    }
    if p >= le && le > ls {
        prev(t, le)
    } else {
        p
    }
}
/// 行内の `c` 列目のバイト位置（行が短ければ行末）。
fn col_byte(t: &str, ls: usize, c: usize) -> usize {
    let le = line_end(t, ls);
    t[ls..le].char_indices().nth(c).map_or(le, |(i, _)| ls + i)
}
/// ノーマルモードではカーソルを改行の上に置かない（空行を除く）。
pub fn clamp_normal(t: &str, p: usize) -> usize {
    let p = p.min(t.len());
    let le = line_end(t, p);
    if p >= le && le > line_start(t, p) {
        prev(t, le)
    } else {
        p
    }
}
fn is_blank_line(t: &str, p: usize) -> bool {
    t[line_start(t, p)..line_end(t, p)].trim().is_empty()
}

/// 単語の種類。日本語は文字の種類（ひらがな・カタカナ・漢字）が変わる所を単語の境目とする。
fn class(c: char, big: bool) -> u8 {
    if c.is_whitespace() {
        return 0;
    }
    if big {
        return 1;
    }
    match c {
        c if c.is_ascii_alphanumeric() || c == '_' => 1,
        c if c.is_ascii() => 2,
        '\u{3040}'..='\u{309F}' => 3,
        '\u{30A0}'..='\u{30FF}' | '\u{31F0}'..='\u{31FF}' | '\u{FF66}'..='\u{FF9F}' => 4,
        '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}' | '々' => 5,
        '\u{3000}'..='\u{303F}' | '\u{FF00}'..='\u{FF65}' => 6,
        _ => 7,
    }
}

fn word_fwd(t: &str, p: usize, big: bool) -> usize {
    let mut i = p;
    let Some(c) = ch(t, i) else { return p };
    let k = class(c, big);
    if k != 0 {
        while ch(t, i).is_some_and(|c| class(c, big) == k) {
            i = next(t, i);
        }
    }
    while let Some(c) = ch(t, i) {
        if c == '\n' {
            let n = next(t, i);
            // 空行は単語として止まる
            if ch(t, n) == Some('\n') {
                return n;
            }
            i = n;
            continue;
        }
        if !c.is_whitespace() {
            break;
        }
        i = next(t, i);
    }
    i
}

fn word_end(t: &str, p: usize, big: bool) -> usize {
    let mut i = next(t, p);
    while ch(t, i).is_some_and(char::is_whitespace) {
        i = next(t, i);
    }
    let Some(c) = ch(t, i) else {
        return prev(t, t.len()).max(p);
    };
    let k = class(c, big);
    while ch(t, next(t, i)).is_some_and(|c| class(c, big) == k) {
        i = next(t, i);
    }
    i
}

fn word_back(t: &str, p: usize, big: bool) -> usize {
    if p == 0 {
        return 0;
    }
    let mut i = prev(t, p);
    while i > 0 && ch(t, i).is_some_and(char::is_whitespace) {
        i = prev(t, i);
    }
    let Some(c) = ch(t, i) else { return i };
    let k = class(c, big);
    while i > 0 && ch(t, prev(t, i)).is_some_and(|c| class(c, big) == k) {
        i = prev(t, i);
    }
    i
}

/// `repeat` は `;` `,` での繰り返し（t / T がすぐ隣の文字で止まり続けないよう、1文字先から探す）。
fn find_in_line(
    t: &str,
    p: usize,
    c: char,
    forward: bool,
    till: bool,
    count: usize,
    repeat: bool,
) -> Option<usize> {
    let (ls, le) = (line_start(t, p), line_end(t, p));
    let mut i = p;
    for k in 0..count {
        let skip = till && (repeat || k > 0);
        if forward {
            let from = next(t, i);
            let start = if skip { next(t, from) } else { from }.min(le);
            i = start + t[start..le].find(c)?;
        } else {
            let end = if skip { prev(t, i) } else { i };
            i = ls + t[ls..end.max(ls)].rfind(c)?;
        }
    }
    Some(match (forward, till) {
        (true, true) => prev(t, i),
        (false, true) => next(t, i),
        _ => i,
    })
}

fn paragraph(t: &str, p: usize, forward: bool) -> usize {
    let mut line = line_no(t, p);
    let total = line_count(t);
    // 今いる空行の並びを抜けてから、次の空行を探す
    let blank = |n: usize| is_blank_line(t, nth_line(t, n));
    if forward {
        while line + 1 < total && blank(line) {
            line += 1;
        }
        while line + 1 < total && !blank(line) {
            line += 1;
        }
        if blank(line) {
            nth_line(t, line)
        } else {
            t.len()
        }
    } else {
        while line > 0 && blank(line) {
            line -= 1;
        }
        while line > 0 && !blank(line) {
            line -= 1;
        }
        nth_line(t, line)
    }
}

const PAIRS: [(char, char); 4] = [('(', ')'), ('[', ']'), ('{', '}'), ('<', '>')];

fn match_pair(t: &str, p: usize) -> Option<usize> {
    let le = line_end(t, p);
    let (start, c) = t[p..le]
        .char_indices()
        .find(|(_, c)| PAIRS.iter().any(|(o, cl)| o == c || cl == c))
        .map(|(i, c)| (p + i, c))?;
    if let Some(&(open, close)) = PAIRS.iter().find(|(o, _)| *o == c) {
        let mut depth = 0;
        for (i, c) in t[start..].char_indices() {
            if c == open {
                depth += 1;
            } else if c == close {
                depth -= 1;
                if depth == 0 {
                    return Some(start + i);
                }
            }
        }
        None
    } else {
        let &(open, close) = PAIRS.iter().find(|(_, cl)| *cl == c)?;
        let mut depth = 0;
        for (i, c) in t[..=start].char_indices().rev() {
            if c == close {
                depth += 1;
            } else if c == open {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
        }
        None
    }
}

/// 対応する括弧の内側（`inner`）または括弧ごとの範囲。
fn bracket_object(
    t: &str,
    p: usize,
    open: char,
    close: char,
    inner: bool,
) -> Option<(usize, usize)> {
    let mut depth = 0;
    let mut left = None;
    let upto = if ch(t, p) == Some(open) {
        next(t, p)
    } else {
        p
    };
    for (i, c) in t[..upto].char_indices().rev() {
        if c == close && i != p {
            depth += 1;
        } else if c == open {
            if depth == 0 {
                left = Some(i);
                break;
            }
            depth -= 1;
        }
    }
    let left = left?;
    let mut depth = 0;
    for (i, c) in t[left..].char_indices() {
        if c == open {
            depth += 1;
        } else if c == close {
            depth -= 1;
            if depth == 0 {
                let right = left + i;
                return Some(if inner {
                    (left + open.len_utf8(), right)
                } else {
                    (left, right + close.len_utf8())
                });
            }
        }
    }
    None
}

fn quote_object(t: &str, p: usize, q: char, inner: bool) -> Option<(usize, usize)> {
    let (ls, le) = (line_start(t, p), line_end(t, p));
    let quotes: Vec<usize> = t[ls..le].match_indices(q).map(|(i, _)| ls + i).collect();
    let (pairs, _) = quotes.as_chunks::<2>();
    let pair = pairs
        .iter()
        .find(|w| w[0] <= p && p <= w[1])
        .or_else(|| pairs.iter().find(|w| w[0] > p))?;
    let (a, b) = (pair[0], pair[1]);
    Some(if inner {
        (a + q.len_utf8(), b)
    } else {
        (a, b + q.len_utf8())
    })
}

fn word_object(t: &str, p: usize, big: bool, around: bool) -> Option<(usize, usize)> {
    let c = ch(t, p).filter(|c| *c != '\n')?;
    let k = class(c, big);
    let (ls, le) = (line_start(t, p), line_end(t, p));
    let mut a = p;
    while a > ls && ch(t, prev(t, a)).is_some_and(|c| class(c, big) == k) {
        a = prev(t, a);
    }
    let mut b = next(t, p);
    while b < le && ch(t, b).is_some_and(|c| class(c, big) == k) {
        b = next(t, b);
    }
    if around && k != 0 {
        let mut e = b;
        while e < le && ch(t, e).is_some_and(|c| c == ' ' || c == '\t') {
            e = next(t, e);
        }
        if e > b {
            b = e;
        } else {
            while a > ls && ch(t, prev(t, a)).is_some_and(|c| c == ' ' || c == '\t') {
                a = prev(t, a);
            }
        }
    }
    Some((a, b))
}

fn paragraph_object(t: &str, p: usize, around: bool) -> (usize, usize) {
    let total = line_count(t);
    let blank = |n: usize| is_blank_line(t, nth_line(t, n));
    let here = line_no(t, p);
    let kind = blank(here);
    let (mut a, mut b) = (here, here);
    while a > 0 && blank(a - 1) == kind {
        a -= 1;
    }
    while b + 1 < total && blank(b + 1) == kind {
        b += 1;
    }
    if around {
        while b + 1 < total && blank(b + 1) != kind {
            b += 1;
        }
    }
    (nth_line(t, a), line_end(t, nth_line(t, b)))
}

/// 行単位の範囲を、改行も含めた削除範囲に広げる。
fn linewise_range(t: &str, a: usize, b: usize) -> (usize, usize) {
    let start = line_start(t, a.min(b));
    let end = line_end(t, a.max(b));
    if end < t.len() {
        (start, end + 1)
    } else if start > 0 {
        // 最終行は、前の行末の改行ごと消す
        (start - 1, end)
    } else {
        (start, end)
    }
}

enum Parsed {
    Incomplete,
    Invalid,
    Done(Action),
}

#[derive(Debug, Clone)]
enum Motion {
    Left,
    Right,
    Up,
    Down,
    WordFwd(bool),
    WordBack(bool),
    WordEnd(bool),
    LineStart,
    FirstNonBlank,
    LineEnd,
    /// gg（行番号の指定があればその行）
    Top,
    /// G
    Bottom,
    Find(char, bool, bool),
    RepeatFind(bool),
    ParaFwd,
    ParaBack,
    Match,
    HalfDown,
    HalfUp,
    /// `'a`（行の最初の文字へ、行単位）/ `` `a ``（その位置へ）
    Mark(char, bool),
}

#[derive(Debug, Clone)]
enum Object {
    Word(bool, bool),
    Paragraph(bool),
    Bracket(char, char, bool),
    Quote(char, bool),
}

#[derive(Debug, Clone)]
enum Target {
    Motion(Motion),
    Object(Object),
    /// dd, cc, yy, >>, <<
    Line,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Delete,
    Change,
    Yank,
    Indent,
    Outdent,
    Lower,
    Upper,
    Toggle,
}

#[derive(Debug, Clone)]
enum Action {
    Move(Motion),
    Operate(Op, Target),
    Insert(char),
    DeleteChar(bool),
    Replace(char),
    Put(bool),
    Join,
    ToggleCase,
    Visual(bool),
    Repeat,
    Undo,
    Find(bool),
    FindNext(bool),
    SearchWord(bool),
    Cmdline,
    /// `qa`: マクロの記録を始める
    Record(char),
    /// `@a` / `@@`: マクロを再生する
    Play(char),
    /// `ma`: マークを付ける
    SetMark(char),
}

/// キー列（数値プレフィックスを除く）を解釈する。
fn parse(keys: &[char]) -> Parsed {
    use Parsed::*;
    let Some(&first) = keys.first() else {
        return Incomplete;
    };
    let rest = &keys[1..];
    if let Some(op) = op_of(keys) {
        let op_len = if first == 'g' { 2 } else { 1 };
        let rest = &keys[op_len..];
        // 演算子のあとの数値は飛ばす（倍数はcountで扱う）
        let digits = rest
            .iter()
            .take_while(|c| c.is_ascii_digit() && **c != '0')
            .count();
        let rest = &rest[digits..];
        let Some(&c) = rest.first() else {
            return Incomplete;
        };
        let same = match op {
            Op::Lower => c == 'u' || c == 'g' && rest.get(1) == Some(&'u'),
            Op::Upper => c == 'U',
            Op::Toggle => c == '~',
            _ => c == first,
        };
        if same {
            return Done(Action::Operate(op, Target::Line));
        }
        if c == 'i' || c == 'a' {
            let Some(&o) = rest.get(1) else {
                return Incomplete;
            };
            let inner = c == 'i';
            let obj = match o {
                'w' => Object::Word(false, !inner),
                'W' => Object::Word(true, !inner),
                'p' => Object::Paragraph(!inner),
                '(' | ')' | 'b' => Object::Bracket('(', ')', inner),
                '[' | ']' => Object::Bracket('[', ']', inner),
                '{' | '}' | 'B' => Object::Bracket('{', '}', inner),
                '<' | '>' => Object::Bracket('<', '>', inner),
                '"' | '\'' | '`' => Object::Quote(o, inner),
                _ => return Invalid,
            };
            return Done(Action::Operate(op, Target::Object(obj)));
        }
        return match motion(rest) {
            Done(Action::Move(m)) => Done(Action::Operate(op, Target::Motion(m))),
            Incomplete => Incomplete,
            _ => Invalid,
        };
    }
    match first {
        'i' | 'a' | 'I' | 'A' | 'o' | 'O' | 's' | 'S' | 'C' => Done(Action::Insert(first)),
        'x' => Done(Action::DeleteChar(true)),
        'X' => Done(Action::DeleteChar(false)),
        'D' => Done(Action::Operate(Op::Delete, Target::Motion(Motion::LineEnd))),
        'Y' => Done(Action::Operate(Op::Yank, Target::Line)),
        'r' => rest
            .first()
            .map_or(Incomplete, |&c| Done(Action::Replace(c))),
        'p' => Done(Action::Put(true)),
        'P' => Done(Action::Put(false)),
        'J' => Done(Action::Join),
        '~' => Done(Action::ToggleCase),
        'v' => Done(Action::Visual(false)),
        'V' => Done(Action::Visual(true)),
        '.' => Done(Action::Repeat),
        'u' => Done(Action::Undo),
        '/' => Done(Action::Find(true)),
        '?' => Done(Action::Find(false)),
        'n' => Done(Action::FindNext(true)),
        'N' => Done(Action::FindNext(false)),
        '*' => Done(Action::SearchWord(true)),
        '#' => Done(Action::SearchWord(false)),
        ':' => Done(Action::Cmdline),
        'q' => rest
            .first()
            .map_or(Incomplete, |&c| Done(Action::Record(c))),
        '@' => rest.first().map_or(Incomplete, |&c| Done(Action::Play(c))),
        'm' => rest
            .first()
            .map_or(Incomplete, |&c| Done(Action::SetMark(c))),
        _ => motion(keys),
    }
}

fn op_of(keys: &[char]) -> Option<Op> {
    Some(match keys {
        ['d', ..] => Op::Delete,
        ['c', ..] => Op::Change,
        ['y', ..] => Op::Yank,
        ['>', ..] => Op::Indent,
        ['<', ..] => Op::Outdent,
        ['g', 'u', ..] => Op::Lower,
        ['g', 'U', ..] => Op::Upper,
        ['g', '~', ..] => Op::Toggle,
        _ => return None,
    })
}

fn motion(keys: &[char]) -> Parsed {
    use Motion::*;
    let Some(&c) = keys.first() else {
        return Parsed::Incomplete;
    };
    let m = match c {
        'h' => Left,
        'l' | ' ' => Right,
        'k' => Up,
        'j' => Down,
        'w' => WordFwd(false),
        'W' => WordFwd(true),
        'b' => WordBack(false),
        'B' => WordBack(true),
        'e' => WordEnd(false),
        'E' => WordEnd(true),
        '0' => LineStart,
        '^' => FirstNonBlank,
        '$' => LineEnd,
        'G' => Bottom,
        '}' => ParaFwd,
        '{' => ParaBack,
        '%' => Match,
        ';' => RepeatFind(false),
        ',' => RepeatFind(true),
        'g' => match keys.get(1) {
            None => return Parsed::Incomplete,
            Some('g') => Top,
            Some(_) => return Parsed::Invalid,
        },
        'f' | 'F' | 't' | 'T' => match keys.get(1) {
            None => return Parsed::Incomplete,
            Some(&x) => Find(x, c == 'f' || c == 't', c == 't' || c == 'T'),
        },
        '\'' | '`' => match keys.get(1) {
            None => return Parsed::Incomplete,
            Some(&x) => Mark(x, c == '`'),
        },
        _ => return Parsed::Invalid,
    };
    Parsed::Done(Action::Move(m))
}

impl Vim {
    pub fn pending(&self) -> String {
        self.keys
            .iter()
            .filter_map(|k| match k {
                Key::Char(c) => Some(*c),
                _ => None,
            })
            .collect()
    }

    /// ステータスバーに出すモード名
    pub fn mode_label(&self) -> &'static str {
        match (self.helix, self.mode) {
            (true, Mode::Visual | Mode::VisualLine) => "SELECT",
            (_, m) => m.label(),
        }
    }

    /// ビジュアルモードの選択範囲（バイト位置、終わりは含まない）。
    /// Helix風ではノーマルモードでも、起点と頭が違えば選択がある。
    pub fn selection(&self, t: &str, cursor: usize) -> Option<(usize, usize)> {
        if self.helix && self.mode != Mode::Insert {
            if self.anchor == cursor && self.mode == Mode::Normal {
                return None;
            }
            let (a, b) = (self.anchor.min(cursor), self.anchor.max(cursor));
            return Some((a, next(t, b).min(t.len()).max(b)));
        }
        match self.mode {
            Mode::Visual => {
                let (a, b) = (self.anchor.min(cursor), self.anchor.max(cursor));
                Some((a, next(t, b).min(t.len()).max(b)))
            }
            Mode::VisualLine => {
                let (a, b) = (self.anchor.min(cursor), self.anchor.max(cursor));
                Some((line_start(t, a), line_end(t, b)))
            }
            _ => None,
        }
    }

    /// 記録中のマクロのレジスタ
    pub fn recording(&self) -> Option<char> {
        self.recording.as_ref().map(|(r, _)| *r)
    }

    /// キーを1つ処理する。
    pub fn handle(&mut self, t: &mut String, cursor: &mut usize, key: Key) -> Effects {
        // マクロの記録（再生中のキーは記録しない）
        if self.macro_depth == 0 {
            if let Some((_, rec)) = self.recording.as_mut() {
                rec.push(key);
            }
        }
        // マークとジャンプリストの位置を本文の変更に合わせて動かすため、変更前の本文を覚えておく
        let before = (!self.marks.is_empty() || !self.jumps.is_empty()).then(|| t.clone());
        let fx = self.handle_inner(t, cursor, key);
        if fx.changed {
            if let Some(old) = before {
                self.follow_edit(&old, t);
            }
        }
        fx
    }

    /// 本文の変更（old → new）に合わせて、マークとジャンプリストの位置を動かす
    fn follow_edit(&mut self, old: &str, new: &str) {
        let (a, b) = (old.as_bytes(), new.as_bytes());
        let prefix = a.iter().zip(b).take_while(|(x, y)| x == y).count();
        let max = a.len().min(b.len()) - prefix;
        let suffix = a
            .iter()
            .rev()
            .zip(b.iter().rev())
            .take(max)
            .take_while(|(x, y)| x == y)
            .count();
        self.shift_positions(prefix, a.len() - prefix - suffix, b.len() - prefix - suffix);
    }

    /// `at` から `removed` バイトが `inserted` バイトに変わった
    fn shift_positions(&mut self, at: usize, removed: usize, inserted: usize) {
        let mv = |x: &mut usize| {
            if *x >= at + removed {
                *x = *x + inserted - removed;
            } else if *x > at {
                *x = at;
            }
        };
        self.marks.values_mut().for_each(mv);
        self.jumps.iter_mut().for_each(mv);
    }

    /// ジャンプする前の位置を、ジャンプリストと `'` マークに入れる
    fn push_jump(&mut self, t: &str, p: usize) {
        self.jumps.truncate(self.jump_at);
        let line = line_no(t, p.min(t.len()));
        self.jumps.retain(|&j| line_no(t, j.min(t.len())) != line);
        self.jumps.push(p);
        if self.jumps.len() > 100 {
            self.jumps.remove(0);
        }
        self.jump_at = self.jumps.len();
        self.marks.insert('\'', p);
    }

    fn handle_inner(&mut self, t: &mut String, cursor: &mut usize, key: Key) -> Effects {
        let mut fx = Effects::default();
        *cursor = (*cursor).min(t.len());
        if let Some(line) = self.cmdline.as_mut() {
            match key {
                Key::Char(c) => line.push(c),
                Key::Backspace => {
                    if line.pop().is_none() {
                        self.cmdline = None;
                    }
                }
                Key::Esc => self.cmdline = None,
                Key::Enter => {
                    let line = self.cmdline.take().unwrap_or_default();
                    self.ex(line.trim(), t, cursor, &mut fx);
                }
                _ => {}
            }
            return fx;
        }
        if self.mode == Mode::Insert {
            if self.helix && key == Key::Esc {
                // Helix は挿入を終えてもカーソルを戻さない
                self.mode = Mode::Normal;
                self.insert_record = None;
                self.anchor = *cursor;
                return fx;
            }
            self.insert_key(t, cursor, key, &mut fx);
            return fx;
        }
        if self.helix {
            return self.helix_key(t, cursor, key);
        }
        match key {
            Key::Esc => {
                self.keys.clear();
                self.pending_register = None;
                if self.mode.is_visual() {
                    self.leave_visual(t, *cursor);
                }
                return fx;
            }
            Key::CtrlO | Key::CtrlI => {
                self.keys.clear();
                let p = *cursor;
                if key == Key::CtrlO && self.jump_at > 0 {
                    if self.jump_at == self.jumps.len() {
                        // 今の位置も入れておき、Ctrl+I で戻れるようにする
                        self.push_jump(t, p);
                        self.jump_at = self.jumps.len() - 1;
                    }
                    self.jump_at -= 1;
                } else if key == Key::CtrlI && self.jump_at + 1 < self.jumps.len() {
                    self.jump_at += 1;
                } else {
                    return fx;
                }
                *cursor = clamp_normal(t, self.jumps[self.jump_at]);
                return fx;
            }
            Key::CtrlV => {
                self.keys.clear();
                if self.mode == Mode::VisualBlock {
                    self.leave_visual(t, *cursor);
                } else {
                    if !self.mode.is_visual() {
                        self.anchor = *cursor;
                    }
                    self.mode = Mode::VisualBlock;
                }
                return fx;
            }
            Key::CtrlR => {
                self.keys.clear();
                fx.cmd = Some(Cmd::Redo(1));
                return fx;
            }
            Key::CtrlD | Key::CtrlU => {
                self.keys.clear();
                let m = if key == Key::CtrlD {
                    Motion::HalfDown
                } else {
                    Motion::HalfUp
                };
                self.run(Action::Move(m), 1, t, cursor, &mut fx);
                return fx;
            }
            Key::Enter => {
                self.keys.clear();
                self.run(Action::Move(Motion::Down), 1, t, cursor, &mut fx);
                *cursor = first_nonblank(t, *cursor);
                return fx;
            }
            Key::Backspace => {
                self.keys.clear();
                self.run(Action::Move(Motion::Left), 1, t, cursor, &mut fx);
                return fx;
            }
            Key::Char(_) => self.keys.push(key),
        }
        // レジスタの指定（`"a`）。次のコマンドが使う
        if self.keys.first() == Some(&Key::Char('"')) {
            match self.keys.get(1) {
                None => return fx,
                Some(&Key::Char(r)) => {
                    self.keys.clear();
                    if r.is_ascii_alphanumeric() || matches!(r, '"' | '_' | '+' | '*') {
                        self.pending_register = Some(r);
                    }
                    return fx;
                }
                Some(_) => {
                    self.keys.clear();
                    return fx;
                }
            }
        }
        // マクロの記録の終わり（記録中の `q`）
        if self.recording.is_some() && self.keys == [Key::Char('q')] {
            self.keys.clear();
            if let Some((r, mut rec)) = self.recording.take() {
                rec.pop(); // 終わりの q
                self.macros.insert(r, rec);
            }
            return fx;
        }
        // 矩形選択だけのキー
        if self.mode == Mode::VisualBlock {
            match self.keys[..] {
                [Key::Char(c @ ('I' | 'A'))] => {
                    self.keys.clear();
                    self.block_insert_start(t, cursor, c == 'A');
                    return fx;
                }
                [Key::Char('r')] => return fx,
                [Key::Char('r'), Key::Char(c)] => {
                    self.keys.clear();
                    self.block_replace(c, t, cursor, &mut fx);
                    return fx;
                }
                _ => {}
            }
        }
        if self.mode.is_visual() && self.keys.len() == 1 {
            let Key::Char(c) = key else { unreachable!() };
            let op = match c {
                'd' | 'x' => Some(Op::Delete),
                'c' | 's' => Some(Op::Change),
                'y' => Some(Op::Yank),
                '>' => Some(Op::Indent),
                '<' => Some(Op::Outdent),
                'u' => Some(Op::Lower),
                'U' => Some(Op::Upper),
                '~' => Some(Op::Toggle),
                _ => None,
            };
            if let Some(op) = op {
                self.keys.clear();
                self.visual_op(op, t, cursor, &mut fx);
                return fx;
            }
            if c == 'o' {
                self.keys.clear();
                std::mem::swap(&mut self.anchor, cursor);
                return fx;
            }
        }
        let chars: Vec<char> = self.pending().chars().collect();
        // 先頭の数値（0は数値の途中だけ）
        let digits = chars
            .iter()
            .enumerate()
            .take_while(|(i, c)| c.is_ascii_digit() && (*i > 0 || **c != '0'))
            .count();
        let count: Option<usize> = chars[..digits].iter().collect::<String>().parse().ok();
        // 演算子のあとの数値も倍数にする（2d3w = 6単語）
        let body = &chars[digits..];
        let inner_count = op_of(body).and_then(|op| {
            let skip = if matches!(op, Op::Lower | Op::Upper | Op::Toggle) {
                2
            } else {
                1
            };
            let s: String = body
                .get(skip..)?
                .iter()
                .take_while(|c| c.is_ascii_digit())
                .collect();
            s.parse::<usize>().ok()
        });
        match parse(body) {
            Parsed::Incomplete => {}
            Parsed::Invalid => self.keys.clear(),
            Parsed::Done(action) => {
                let keys = std::mem::take(&mut self.keys);
                let n = count.unwrap_or(1) * inner_count.unwrap_or(1);
                // gg / G に数値が付いたら行番号
                let action = match (action, count) {
                    (Action::Move(Motion::Top | Motion::Bottom), Some(line)) => {
                        self.push_jump(t, *cursor);
                        *cursor = first_nonblank(t, nth_line(t, line.saturating_sub(1)));
                        self.want_col = None;
                        return fx;
                    }
                    (a, _) => a,
                };
                let before_mode = self.mode;
                let records = self.records(&action);
                self.run(action, n, t, cursor, &mut fx);
                if records && !self.replaying && before_mode == Mode::Normal {
                    if self.mode == Mode::Insert {
                        if let Some(rec) = self.insert_record.as_mut() {
                            rec.0 = keys;
                        }
                    } else {
                        self.last_change = keys;
                    }
                }
            }
        }
        fx
    }

    /// ビジュアルモードを抜ける（`'<` `'>` に選択の範囲を覚える）
    fn leave_visual(&mut self, t: &str, cursor: usize) {
        let (a, b) = (self.anchor.min(cursor), self.anchor.max(cursor));
        self.marks.insert('<', a.min(t.len()));
        self.marks.insert('>', b.min(t.len()));
        self.mode = Mode::Normal;
    }

    /// 矩形選択の範囲: (最初の行, 最後の行, 左の列, 右の列)。列は文字数で、右の列も含む
    fn block_rect(&self, t: &str, cursor: usize) -> (usize, usize, usize, usize) {
        let (a, b) = (self.anchor.min(t.len()), cursor.min(t.len()));
        let (la, lb) = (line_no(t, a), line_no(t, b));
        let (ca, cb) = (col(t, a), col(t, b));
        (la.min(lb), la.max(lb), ca.min(cb), ca.max(cb))
    }

    /// 矩形選択の、各行の部分（バイト範囲。行が短ければ空）
    pub fn block_ranges(&self, t: &str, cursor: usize) -> Vec<(usize, usize)> {
        if self.mode != Mode::VisualBlock || self.helix {
            return Vec::new();
        }
        let (l0, l1, c0, c1) = self.block_rect(t, cursor);
        (l0..=l1)
            .map(|l| {
                let ls = nth_line(t, l);
                (col_byte(t, ls, c0), col_byte(t, ls, c1 + 1))
            })
            .collect()
    }

    fn block_op(&mut self, op: Op, t: &mut String, cursor: &mut usize, fx: &mut Effects) {
        let (l0, l1, c0, _) = self.block_rect(t, *cursor);
        let ranges = self.block_ranges(t, *cursor);
        self.leave_visual(t, *cursor);
        let top = |t: &str| col_byte(t, nth_line(t, l0), c0);
        match op {
            Op::Yank | Op::Delete | Op::Change => {
                let text = ranges
                    .iter()
                    .map(|&(a, b)| &t[a..b])
                    .collect::<Vec<_>>()
                    .join("\n");
                let reg = Register {
                    text,
                    linewise: false,
                    block: true,
                };
                self.store_register(reg, op == Op::Yank, fx);
                if op == Op::Yank {
                    *cursor = top(t);
                    return;
                }
                for &(a, b) in ranges.iter().rev() {
                    t.replace_range(a..b, "");
                }
                fx.changed = true;
                if op == Op::Change {
                    *cursor = top(t);
                    self.enter_insert(t, *cursor);
                    self.block_insert = (l1 > l0).then_some(BlockInsert {
                        lines: (l0 + 1, l1),
                        col: c0,
                        pad: false,
                    });
                } else {
                    *cursor = clamp_normal(t, top(t));
                }
            }
            Op::Indent | Op::Outdent => {
                let (a, b) = (nth_line(t, l0), line_end(t, nth_line(t, l1)));
                self.apply(op, a, b, true, t, cursor, fx);
            }
            Op::Lower | Op::Upper | Op::Toggle => {
                for &(a, b) in ranges.iter().rev() {
                    let src = &t[a..b];
                    let out = match op {
                        Op::Lower => src.to_lowercase(),
                        Op::Upper => src.to_uppercase(),
                        _ => toggle_case(src),
                    };
                    if out != src {
                        t.replace_range(a..b, &out);
                        fx.changed = true;
                    }
                }
                *cursor = clamp_normal(t, top(t));
            }
        }
    }

    /// 矩形選択の `I`（左の列の前）/ `A`（右の列の後ろ）。最初の行で入力し、Esc で残りの行にも入れる
    fn block_insert_start(&mut self, t: &mut String, cursor: &mut usize, append: bool) {
        let (l0, l1, c0, c1) = self.block_rect(t, *cursor);
        self.leave_visual(t, *cursor);
        let col = if append { c1 + 1 } else { c0 };
        let ls = nth_line(t, l0);
        let len = t[ls..line_end(t, ls)].chars().count();
        if append && len < col {
            let le = line_end(t, ls);
            t.insert_str(le, &" ".repeat(col - len));
        }
        *cursor = col_byte(t, ls, col);
        self.enter_insert(t, *cursor);
        self.block_insert = (l1 > l0).then_some(BlockInsert {
            lines: (l0 + 1, l1),
            col,
            pad: append,
        });
    }

    /// 矩形選択の `r`: 選んだ文字をすべて `c` にする
    fn block_replace(&mut self, c: char, t: &mut String, cursor: &mut usize, fx: &mut Effects) {
        let (l0, _, c0, _) = self.block_rect(t, *cursor);
        let ranges = self.block_ranges(t, *cursor);
        self.leave_visual(t, *cursor);
        for &(a, b) in ranges.iter().rev() {
            let n = t[a..b].chars().count();
            if n > 0 {
                t.replace_range(a..b, &c.to_string().repeat(n));
                fx.changed = true;
            }
        }
        *cursor = clamp_normal(t, col_byte(t, nth_line(t, l0), c0));
    }

    /// 矩形でヤンクしたものを、カーソルの列から下の行へ1行ずつ貼る
    fn block_put(
        &mut self,
        reg: &Register,
        after: bool,
        n: usize,
        t: &mut String,
        cursor: &mut usize,
    ) {
        let p = *cursor;
        let line = line_no(t, p);
        let c = col(t, p) + usize::from(after && line_end(t, p) > p);
        for (i, piece) in reg.text.split('\n').enumerate() {
            let target = line + i;
            while line_count(t) <= target {
                t.push('\n');
            }
            let ls = nth_line(t, target);
            let le = line_end(t, ls);
            let len = t[ls..le].chars().count();
            if len < c {
                t.insert_str(le, &" ".repeat(c - len));
            }
            let at = col_byte(t, nth_line(t, target), c);
            t.insert_str(at, &piece.repeat(n.max(1)));
        }
        *cursor = col_byte(t, nth_line(t, line), c);
    }

    fn records(&self, a: &Action) -> bool {
        match a {
            Action::Operate(op, _) => *op != Op::Yank,
            Action::Insert(_)
            | Action::DeleteChar(_)
            | Action::Replace(_)
            | Action::Put(_)
            | Action::Join
            | Action::ToggleCase => true,
            _ => false,
        }
    }

    fn enter_insert(&mut self, t: &str, at: usize) {
        self.mode = Mode::Insert;
        self.insert_record = Some((Vec::new(), at, t.len()));
    }

    fn insert_key(&mut self, t: &mut String, cursor: &mut usize, key: Key, fx: &mut Effects) {
        match key {
            Key::Esc => {
                self.mode = Mode::Normal;
                let block = self.block_insert.take();
                if let Some((mut keys, start, len)) = self.insert_record.take() {
                    // 続けて入力した文字列（エディタが入れたもの）
                    let grown = t.len().saturating_sub(len);
                    let typed = t
                        .get(start..start + grown)
                        .filter(|_| *cursor == start + grown)
                        .map(str::to_string);
                    let as_keys = |typed: &str| -> Vec<Key> {
                        typed
                            .chars()
                            .map(|c| if c == '\n' { Key::Enter } else { Key::Char(c) })
                            .collect()
                    };
                    if self.macro_depth == 0 {
                        // マクロの記録にも、入力した文字を入れる（Esc の前）
                        if let (Some((_, rec)), Some(typed)) = (self.recording.as_mut(), &typed) {
                            let at = rec.len().saturating_sub(1);
                            rec.splice(at..at, as_keys(typed));
                        }
                        if grown > 0 && typed.is_some() {
                            self.shift_positions(start, 0, grown);
                        }
                    }
                    if !self.replaying {
                        // 挿入した文字列を記録して、`.` で再生できるようにする
                        if let Some(typed) = &typed {
                            keys.extend(as_keys(typed));
                        }
                        keys.push(Key::Esc);
                        self.last_change = keys;
                    }
                    // 矩形選択からの挿入: 残りの行の同じ列にも入れる
                    if let (Some(b), Some(typed)) = (block, typed.filter(|s| !s.contains('\n'))) {
                        if !typed.is_empty() {
                            for l in b.lines.0..=b.lines.1.min(line_count(t) - 1) {
                                let ls = nth_line(t, l);
                                let le = line_end(t, ls);
                                let len = t[ls..le].chars().count();
                                if len < b.col {
                                    if !b.pad {
                                        continue;
                                    }
                                    t.insert_str(le, &" ".repeat(b.col - len));
                                }
                                let at = col_byte(t, nth_line(t, l), b.col);
                                t.insert_str(at, &typed);
                            }
                            fx.changed = true;
                            *cursor = start;
                            return;
                        }
                    }
                }
                if *cursor > line_start(t, *cursor) {
                    *cursor = prev(t, *cursor);
                }
            }
            // ここから下は `.` の再生のときだけ使う（普段の入力はエディタが行う）
            Key::Char(c) => {
                t.insert(*cursor, c);
                *cursor += c.len_utf8();
                fx.changed = true;
            }
            Key::Enter => {
                t.insert(*cursor, '\n');
                *cursor += 1;
                fx.changed = true;
            }
            Key::Backspace if *cursor > 0 => {
                let p = prev(t, *cursor);
                t.replace_range(p..*cursor, "");
                *cursor = p;
                fx.changed = true;
            }
            _ => {}
        }
    }

    /// 移動先と種類。
    fn motion_target(&mut self, m: &Motion, n: usize, t: &str, p: usize) -> Option<(usize, Kind)> {
        use Motion::*;
        let vertical = matches!(m, Up | Down | HalfDown | HalfUp);
        if !vertical {
            self.want_col = None;
        }
        let r = match m {
            Left => {
                let ls = line_start(t, p);
                let mut i = p;
                for _ in 0..n {
                    if i > ls {
                        i = prev(t, i);
                    }
                }
                (i, Kind::Exclusive)
            }
            Right => {
                let le = line_end(t, p);
                let mut i = p;
                for _ in 0..n {
                    if next(t, i) <= le {
                        i = next(t, i);
                    }
                }
                (i, Kind::Exclusive)
            }
            Up | Down | HalfDown | HalfUp => {
                let want = *self.want_col.get_or_insert_with(|| col(t, p));
                let line = line_no(t, p);
                let n = if matches!(m, HalfDown | HalfUp) {
                    15 * n
                } else {
                    n
                };
                let target = if matches!(m, Down | HalfDown) {
                    (line + n).min(line_count(t) - 1)
                } else {
                    line.saturating_sub(n)
                };
                if target == line && !matches!(m, HalfDown | HalfUp) {
                    return None;
                }
                (at_col(t, nth_line(t, target), want), Kind::Linewise)
            }
            WordFwd(big) => {
                let mut i = p;
                for _ in 0..n {
                    i = word_fwd(t, i, *big);
                }
                (i, Kind::Exclusive)
            }
            WordBack(big) => {
                let mut i = p;
                for _ in 0..n {
                    i = word_back(t, i, *big);
                }
                (i, Kind::Exclusive)
            }
            WordEnd(big) => {
                let mut i = p;
                for _ in 0..n {
                    i = word_end(t, i, *big);
                }
                (i, Kind::Inclusive)
            }
            LineStart => (line_start(t, p), Kind::Exclusive),
            FirstNonBlank => (first_nonblank(t, p), Kind::Exclusive),
            LineEnd => {
                let mut i = p;
                for _ in 1..n {
                    i = next(t, line_end(t, i)).min(t.len());
                }
                (
                    prev(t, line_end(t, i)).max(line_start(t, i)),
                    Kind::Inclusive,
                )
            }
            Top => (first_nonblank(t, 0), Kind::Linewise),
            Bottom => (first_nonblank(t, line_start(t, t.len())), Kind::Linewise),
            Find(c, fwd, till) => {
                self.last_find = Some((*c, *fwd, *till));
                let target = find_in_line(t, p, *c, *fwd, *till, n, false)?;
                (
                    target,
                    if *fwd {
                        Kind::Inclusive
                    } else {
                        Kind::Exclusive
                    },
                )
            }
            RepeatFind(reverse) => {
                let (c, fwd, till) = self.last_find?;
                let fwd = fwd != *reverse;
                let target = find_in_line(t, p, c, fwd, till, n, true)?;
                (
                    target,
                    if fwd {
                        Kind::Inclusive
                    } else {
                        Kind::Exclusive
                    },
                )
            }
            ParaFwd => {
                let mut i = p;
                for _ in 0..n {
                    i = paragraph(t, i, true);
                }
                (i, Kind::Exclusive)
            }
            ParaBack => {
                let mut i = p;
                for _ in 0..n {
                    i = paragraph(t, i, false);
                }
                (i, Kind::Exclusive)
            }
            Match => (match_pair(t, p)?, Kind::Inclusive),
            Mark(c, exact) => {
                let key = if matches!(c, '\'' | '`') { '\'' } else { *c };
                let pos = (*self.marks.get(&key)?).min(t.len());
                if *exact {
                    (pos, Kind::Exclusive)
                } else {
                    (first_nonblank(t, pos), Kind::Linewise)
                }
            }
        };
        Some(r)
    }

    fn object_range(&self, o: &Object, t: &str, p: usize) -> Option<(usize, usize, bool)> {
        Some(match o {
            Object::Word(big, around) => {
                let (a, b) = word_object(t, p, *big, *around)?;
                (a, b, false)
            }
            Object::Paragraph(around) => {
                let (a, b) = paragraph_object(t, p, *around);
                (a, b, true)
            }
            Object::Bracket(o, c, inner) => {
                let (a, b) = bracket_object(t, p, *o, *c, *inner)?;
                (a, b, false)
            }
            Object::Quote(q, inner) => {
                let (a, b) = quote_object(t, p, *q, *inner)?;
                (a, b, false)
            }
        })
    }

    fn run(
        &mut self,
        action: Action,
        n: usize,
        t: &mut String,
        cursor: &mut usize,
        fx: &mut Effects,
    ) {
        let p = *cursor;
        let visual = self.mode.is_visual();
        match action {
            Action::Move(m) => {
                let jump = matches!(
                    m,
                    Motion::Top
                        | Motion::Bottom
                        | Motion::Match
                        | Motion::ParaFwd
                        | Motion::ParaBack
                        | Motion::Mark(..)
                );
                if let Some((target, _)) = self.motion_target(&m, n, t, p) {
                    if jump && target != p {
                        self.push_jump(t, p);
                    }
                    *cursor = if visual {
                        target.min(t.len())
                    } else {
                        clamp_normal(t, target)
                    };
                }
            }
            Action::Operate(op, target) if visual => {
                let _ = target;
                self.visual_op(op, t, cursor, fx);
            }
            Action::Operate(op, target) => {
                let range = match &target {
                    Target::Line => {
                        let last = nth_line(t, (line_no(t, p) + n - 1).min(line_count(t) - 1));
                        Some((line_start(t, p), line_end(t, last), true))
                    }
                    Target::Object(o) => self.object_range(o, t, p),
                    Target::Motion(m) => {
                        let mut m = m.clone();
                        // cw は ce と同じ（単語の後ろの空白は残す）
                        if op == Op::Change
                            && matches!(m, Motion::WordFwd(_))
                            && ch(t, p).is_some_and(|c| !c.is_whitespace())
                        {
                            m = match m {
                                Motion::WordFwd(big) => Motion::WordEnd(big),
                                other => other,
                            };
                            if let Some(c) = ch(t, p) {
                                let big = matches!(m, Motion::WordEnd(true));
                                // カーソルが単語の最後の文字なら、その文字だけ
                                if !ch(t, next(t, p))
                                    .is_some_and(|d| class(d, big) == class(c, big))
                                    && n == 1
                                {
                                    self.apply(op, p, next(t, p), false, t, cursor, fx);
                                    return;
                                }
                            }
                        }
                        self.motion_target(&m, n, t, p).map(|(target, kind)| {
                            let (a, b) = (p.min(target), p.max(target));
                            match kind {
                                Kind::Linewise => (line_start(t, a), line_end(t, b), true),
                                Kind::Inclusive => (
                                    a,
                                    next(t, b).min(line_end(t, b).max(next(t, b).min(t.len()))),
                                    false,
                                ),
                                Kind::Exclusive => {
                                    // dw などで行をまたいだら、その行の終わりまでにする
                                    let b = if matches!(m, Motion::WordFwd(_))
                                        && line_no(t, b) > line_no(t, a)
                                    {
                                        line_end(t, a)
                                    } else {
                                        b
                                    };
                                    (a, b, false)
                                }
                            }
                        })
                    }
                };
                if let Some((a, b, linewise)) = range {
                    let keep = matches!(target, Target::Line) && op == Op::Yank;
                    self.apply(op, a, b, linewise, t, cursor, fx);
                    if keep {
                        *cursor = p;
                    }
                }
            }
            Action::Insert(c) => {
                let (ls, le) = (line_start(t, p), line_end(t, p));
                let at = match c {
                    'i' => p,
                    'a' => next(t, p).min(le),
                    'I' => first_nonblank(t, p),
                    'A' => le,
                    'o' => {
                        let indent = t[ls..first_nonblank(t, p)].to_string();
                        t.insert_str(le, &format!("\n{indent}"));
                        fx.changed = true;
                        le + 1 + indent.len()
                    }
                    'O' => {
                        let indent = t[ls..first_nonblank(t, p)].to_string();
                        t.insert_str(ls, &format!("{indent}\n"));
                        fx.changed = true;
                        ls + indent.len()
                    }
                    's' => {
                        let mut e = p;
                        for _ in 0..n {
                            if e < le {
                                e = next(t, e);
                            }
                        }
                        self.yank_text(&t[p..e], false, false, fx);
                        t.replace_range(p..e, "");
                        fx.changed |= e > p;
                        p
                    }
                    'S' => {
                        let fnb = first_nonblank(t, p);
                        self.yank_text(&t[ls..le], true, false, fx);
                        t.replace_range(fnb..le, "");
                        fx.changed = true;
                        fnb
                    }
                    'C' => {
                        self.yank_text(&t[p..le], false, false, fx);
                        t.replace_range(p..le, "");
                        fx.changed |= le > p;
                        p
                    }
                    _ => p,
                };
                *cursor = at;
                self.enter_insert(t, at);
            }
            Action::DeleteChar(forward) => {
                let (ls, le) = (line_start(t, p), line_end(t, p));
                let (mut a, mut b) = (p, p);
                for _ in 0..n {
                    if forward {
                        if b < le {
                            b = next(t, b);
                        }
                    } else if a > ls {
                        a = prev(t, a);
                    }
                }
                if b > a {
                    self.yank_text(&t[a..b], false, false, fx);
                    t.replace_range(a..b, "");
                    fx.changed = true;
                }
                *cursor = clamp_normal(t, a);
            }
            Action::Replace(c) => {
                let le = line_end(t, p);
                let mut b = p;
                for _ in 0..n {
                    b = next(t, b);
                }
                if b <= le && b > p {
                    let rep: String = std::iter::repeat_n(c, n).collect();
                    t.replace_range(p..b, &rep);
                    fx.changed = true;
                    *cursor = p + rep.len() - c.len_utf8();
                }
            }
            Action::Put(after) => {
                let reg = match self.pending_register.take() {
                    None | Some('"' | '+' | '*') => self.register.clone(),
                    Some('_') => return,
                    Some(r) => self
                        .registers
                        .get(&r.to_ascii_lowercase())
                        .cloned()
                        .unwrap_or_default(),
                };
                if reg.text.is_empty() {
                    return;
                }
                if reg.block {
                    self.block_put(&reg, after, n, t, cursor);
                    fx.changed = true;
                    return;
                }
                let body = reg.text.repeat(n);
                if reg.linewise {
                    let body = body.strip_suffix('\n').unwrap_or(&body).to_string();
                    let at = if after {
                        let le = line_end(t, p);
                        t.insert_str(le, &format!("\n{body}"));
                        le + 1
                    } else {
                        let ls = line_start(t, p);
                        t.insert_str(ls, &format!("{body}\n"));
                        ls
                    };
                    *cursor = first_nonblank(t, at);
                } else {
                    let at = if after && line_end(t, p) > p {
                        next(t, p)
                    } else {
                        p
                    };
                    t.insert_str(at, &body);
                    *cursor = prev(t, at + body.len()).max(at);
                }
                fx.changed = true;
            }
            Action::Join => {
                for _ in 0..n.max(1) {
                    let le = line_end(t, *cursor);
                    if le >= t.len() {
                        break;
                    }
                    let next_start = le + 1;
                    let fnb = first_nonblank(t, next_start);
                    let next_empty = line_end(t, next_start) == fnb;
                    let sep = if next_empty || t[..le].ends_with(' ') {
                        ""
                    } else {
                        " "
                    };
                    t.replace_range(le..fnb, sep);
                    fx.changed = true;
                    *cursor = le;
                }
            }
            Action::ToggleCase => {
                let le = line_end(t, p);
                let mut e = p;
                for _ in 0..n {
                    if e < le {
                        e = next(t, e);
                    }
                }
                let toggled = toggle_case(&t[p..e]);
                if toggled != t[p..e] {
                    t.replace_range(p..e, &toggled);
                    fx.changed = true;
                }
                *cursor = clamp_normal(t, p + toggled.len());
            }
            Action::Visual(line) => {
                let mode = if line { Mode::VisualLine } else { Mode::Visual };
                if self.mode == mode {
                    self.leave_visual(t, p);
                } else {
                    if !visual {
                        self.anchor = p;
                    }
                    self.mode = mode;
                }
            }
            Action::Repeat => {
                let keys = self.last_change.clone();
                if keys.is_empty() {
                    return;
                }
                self.replaying = true;
                for _ in 0..n {
                    for &k in &keys {
                        let r = self.handle(t, cursor, k);
                        fx.changed |= r.changed;
                    }
                    if self.mode == Mode::Insert {
                        let r = self.handle(t, cursor, Key::Esc);
                        fx.changed |= r.changed;
                    }
                }
                self.replaying = false;
            }
            Action::Undo => fx.cmd = Some(Cmd::Undo(n)),
            Action::Find(fwd) => fx.cmd = Some(Cmd::OpenFind(fwd)),
            Action::FindNext(fwd) => fx.cmd = Some(if fwd { Cmd::FindNext } else { Cmd::FindPrev }),
            Action::SearchWord(fwd) => {
                if let Some((a, b)) = word_object(t, p, false, false) {
                    let w = t[a..b].trim();
                    if !w.is_empty() {
                        fx.cmd = Some(Cmd::SearchWord(w.to_string(), fwd));
                    }
                }
            }
            Action::Cmdline => {
                if visual {
                    // ビジュアルモードの範囲を、行の範囲として渡す
                    self.leave_visual(t, p);
                    self.cmdline = Some("'<,'>".to_string());
                } else {
                    self.cmdline = Some(String::new());
                }
            }
            Action::Record(r) => {
                if r.is_ascii_alphanumeric() || r == '"' {
                    let lower = r.to_ascii_lowercase();
                    // 大文字なら、今あるマクロに続けて記録する
                    let keys = if r.is_ascii_uppercase() {
                        self.macros.get(&lower).cloned().unwrap_or_default()
                    } else {
                        Vec::new()
                    };
                    self.recording = Some((lower, keys));
                }
            }
            Action::Play(r) => {
                let r = if r == '@' {
                    let Some(last) = self.last_macro else { return };
                    last
                } else {
                    r.to_ascii_lowercase()
                };
                // 記録したマクロが無ければ、レジスタの文字列をキーとして使う
                let keys = self.macros.get(&r).cloned().or_else(|| {
                    self.registers.get(&r).map(|reg| {
                        reg.text
                            .chars()
                            .map(|c| if c == '\n' { Key::Enter } else { Key::Char(c) })
                            .collect()
                    })
                });
                let Some(keys) = keys else { return };
                if self.macro_depth >= 20 {
                    return;
                }
                self.last_macro = Some(r);
                self.macro_depth += 1;
                for _ in 0..n {
                    for &k in &keys {
                        let r = self.handle(t, cursor, k);
                        fx.changed |= r.changed;
                        if r.yank.is_some() {
                            fx.yank = r.yank;
                        }
                        if r.cmd.is_some() {
                            fx.cmd = r.cmd;
                        }
                    }
                }
                self.macro_depth -= 1;
            }
            Action::SetMark(m) => {
                if m.is_ascii_alphabetic() || matches!(m, '\'' | '`') {
                    let m = if m == '`' { '\'' } else { m };
                    self.marks.insert(m, p);
                }
            }
        }
    }

    /// 消した / ヤンクした文字列をレジスタに入れる（`is_yank` はヤンクか）
    fn yank_text(&mut self, s: &str, linewise: bool, is_yank: bool, fx: &mut Effects) {
        let mut text = s.to_string();
        if linewise && !text.ends_with('\n') {
            text.push('\n');
        }
        let reg = Register {
            text,
            linewise,
            block: false,
        };
        self.store_register(reg, is_yank, fx);
    }

    /// `"a` で指定したレジスタ（無ければ無名のレジスタ）に入れる。
    /// 大文字（`"A`）は追加、`"_` は捨てる。ヤンクは `"0` にも入れる。システムのクリップボードにも入れる
    fn store_register(&mut self, reg: Register, is_yank: bool, fx: &mut Effects) {
        let target = self.pending_register.take();
        let reg = match target {
            Some('_') => return,
            Some(c) if c.is_ascii_lowercase() => {
                self.registers.insert(c, reg.clone());
                reg
            }
            Some(c) if c.is_ascii_uppercase() => {
                let e = self.registers.entry(c.to_ascii_lowercase()).or_default();
                if e.linewise || reg.linewise {
                    if !e.text.is_empty() && !e.text.ends_with('\n') {
                        e.text.push('\n');
                    }
                    e.text.push_str(&reg.text);
                    if !e.text.ends_with('\n') {
                        e.text.push('\n');
                    }
                    e.linewise = true;
                } else {
                    e.text.push_str(&reg.text);
                }
                e.block = false;
                e.clone()
            }
            _ => reg,
        };
        if is_yank && matches!(target, None | Some('"' | '+' | '*')) {
            self.registers.insert('0', reg.clone());
        }
        fx.yank = Some(reg.text.clone());
        self.register = reg;
    }

    /// 演算子を範囲に適用する。`b` は含まない。行単位なら a..b は行の内容（改行を含まない）。
    #[allow(clippy::too_many_arguments)]
    fn apply(
        &mut self,
        op: Op,
        a: usize,
        b: usize,
        linewise: bool,
        t: &mut String,
        cursor: &mut usize,
        fx: &mut Effects,
    ) {
        let (a, b) = (a.min(t.len()), b.min(t.len()));
        match op {
            Op::Yank => {
                self.yank_text(&t[a..b], linewise, true, fx);
                *cursor = a;
            }
            Op::Delete => {
                self.yank_text(&t[a..b], linewise, false, fx);
                if linewise {
                    let (s, e) = linewise_range(t, a, b.max(a));
                    t.replace_range(s..e, "");
                    *cursor = first_nonblank(
                        t,
                        s.min(t.len())
                            .max(if s > 0 && s == t.len() {
                                line_start(t, s)
                            } else {
                                0
                            })
                            .min(t.len()),
                    );
                    if s >= t.len() {
                        *cursor = first_nonblank(t, line_start(t, t.len()));
                    }
                } else {
                    t.replace_range(a..b, "");
                    *cursor = clamp_normal(t, a);
                }
                fx.changed = true;
            }
            Op::Change => {
                self.yank_text(&t[a..b], linewise, false, fx);
                let start = if linewise { first_nonblank(t, a) } else { a };
                let end = if linewise { line_end(t, b) } else { b };
                t.replace_range(start..end, "");
                fx.changed |= end > start;
                *cursor = start;
                self.enter_insert(t, start);
            }
            Op::Indent | Op::Outdent => {
                let unit = indent_unit(t);
                let (s, e) = (line_start(t, a), line_end(t, b.max(a)));
                let changed: Vec<String> = t[s..e]
                    .split('\n')
                    .map(|line| {
                        if op == Op::Indent {
                            if line.is_empty() {
                                String::new()
                            } else {
                                format!("{unit}{line}")
                            }
                        } else if let Some(rest) = line.strip_prefix(&unit) {
                            rest.to_string()
                        } else {
                            line.trim_start_matches([' ', '\t']).to_string()
                        }
                    })
                    .collect();
                let joined = changed.join("\n");
                if joined != t[s..e] {
                    t.replace_range(s..e, &joined);
                    fx.changed = true;
                }
                *cursor = first_nonblank(t, s);
            }
            Op::Lower | Op::Upper | Op::Toggle => {
                let src = &t[a..b];
                let out = match op {
                    Op::Lower => src.to_lowercase(),
                    Op::Upper => src.to_uppercase(),
                    _ => toggle_case(src),
                };
                if out != src {
                    t.replace_range(a..b, &out);
                    fx.changed = true;
                }
                *cursor = clamp_normal(t, a);
            }
        }
    }

    fn visual_op(&mut self, op: Op, t: &mut String, cursor: &mut usize, fx: &mut Effects) {
        if self.mode == Mode::VisualBlock {
            return self.block_op(op, t, cursor, fx);
        }
        let Some((a, b)) = self.selection(t, *cursor) else {
            return;
        };
        let linewise = self.mode == Mode::VisualLine;
        self.leave_visual(t, *cursor);
        self.apply(op, a, b, linewise, t, cursor, fx);
    }

    /// `:` コマンド。
    fn ex(&mut self, line: &str, t: &mut String, cursor: &mut usize, fx: &mut Effects) {
        fx.cmd = Some(match line {
            "w" | "w!" => Cmd::Save,
            "q" | "q!" => Cmd::Close,
            "wq" | "x" | "wq!" => Cmd::SaveClose,
            "noh" | "nohl" | "nohlsearch" => Cmd::ClearSearch,
            "" => return,
            _ => {
                let (range, rest) = self.ex_range(line, t, *cursor);
                let rest = rest.trim_start();
                if rest.is_empty() {
                    // `:12` などは、その行へ移る
                    if let Some((_, last)) = range {
                        self.push_jump(t, *cursor);
                        *cursor = first_nonblank(t, nth_line(t, last));
                        return;
                    }
                }
                let sub = rest
                    .strip_prefix("substitute")
                    .or_else(|| rest.strip_prefix('s'))
                    .filter(|a| {
                        a.chars()
                            .next()
                            .is_some_and(|d| !d.is_alphanumeric() && d != ' ')
                    });
                match sub {
                    Some(args) => {
                        let cur = line_no(t, *cursor);
                        let range = range.unwrap_or((cur, cur));
                        match self.substitute(range, args, t, cursor) {
                            Ok(()) => {
                                fx.changed = true;
                                return;
                            }
                            Err(message) => Cmd::Message(message),
                        }
                    }
                    None => Cmd::Unknown(line.to_string()),
                }
            }
        });
    }

    /// `:` コマンドの行の範囲（0始まりの行番号、両端を含む）と、残りの文字列。
    /// `%`、`12`、`.`、`$`、`'a`（マーク）、`'<,'>`、`+2` などと、それを `,` でつないだもの
    fn ex_range<'a>(
        &self,
        s: &'a str,
        t: &str,
        cursor: usize,
    ) -> (Option<(usize, usize)>, &'a str) {
        let last = line_count(t) - 1;
        if let Some(rest) = s.strip_prefix('%') {
            return (Some((0, last)), rest);
        }
        let Some((a, rest)) = self.ex_address(s, t, cursor) else {
            return (None, s);
        };
        if let Some((b, rest)) = rest
            .strip_prefix(',')
            .and_then(|r| self.ex_address(r, t, cursor))
        {
            return (Some((a.min(b), a.max(b))), rest);
        }
        (Some((a, a)), rest)
    }

    fn ex_address<'a>(&self, s: &'a str, t: &str, cursor: usize) -> Option<(usize, &'a str)> {
        let last = line_count(t) - 1;
        let cur = line_no(t, cursor.min(t.len()));
        let digits = s.chars().take_while(char::is_ascii_digit).count();
        let (mut line, mut rest) = if digits > 0 {
            (
                s[..digits].parse::<usize>().ok()?.saturating_sub(1),
                &s[digits..],
            )
        } else if let Some(r) = s.strip_prefix('.') {
            (cur, r)
        } else if let Some(r) = s.strip_prefix('$') {
            (last, r)
        } else if let Some(r) = s.strip_prefix('\'') {
            let m = r.chars().next()?;
            let pos = *self.marks.get(&m)?;
            (line_no(t, pos.min(t.len())), &r[m.len_utf8()..])
        } else if s.starts_with(['+', '-']) {
            (cur, s)
        } else {
            return None;
        };
        // `+2` / `-1` のずらし
        while let Some(sign) = rest.chars().next().filter(|c| matches!(c, '+' | '-')) {
            let r = &rest[1..];
            let n = r.chars().take_while(char::is_ascii_digit).count();
            let by = if n == 0 {
                1
            } else {
                r[..n].parse::<usize>().ok()?
            };
            line = if sign == '+' {
                line + by
            } else {
                line.saturating_sub(by)
            };
            rest = &r[n..];
        }
        Some((line.min(last), rest))
    }

    /// `:s/パターン/置き換え/フラグ`。パターンは Rust（regex クレート）の正規表現で、
    /// Vim の `\<` `\>` は単語の境目として読む。置き換えの `&` と `\0` は一致した全体、`\1`〜`\9` はグループ。
    /// フラグは `g`（行のすべて）・`i`（大文字小文字を区別しない）・`I`（区別する）
    fn substitute(
        &mut self,
        (l0, l1): (usize, usize),
        args: &str,
        t: &mut String,
        cursor: &mut usize,
    ) -> Result<(), String> {
        let delim = args.chars().next().ok_or("パターンがありません")?;
        let parts = split_unescaped(&args[delim.len_utf8()..], delim);
        let pat = parts
            .first()
            .filter(|p| !p.is_empty())
            .ok_or("パターンがありません")?;
        let replacement = vim_replacement(parts.get(1).map_or("", String::as_str));
        let flags = parts.get(2).map_or("", String::as_str);
        let pattern = pat.replace("\\<", "\\b").replace("\\>", "\\b");
        let re = regex::RegexBuilder::new(&pattern)
            .case_insensitive(flags.contains('i') && !flags.contains('I'))
            .build()
            .map_err(|e| format!("パターンが正しくありません: {e}"))?;
        let all = flags.contains('g');
        let start = nth_line(t, l0);
        let end = line_end(t, nth_line(t, l1));
        let mut out = String::with_capacity(end - start);
        let mut last = None;
        for (i, line) in t[start..end].split('\n').enumerate() {
            if i > 0 {
                out.push('\n');
            }
            if re.is_match(line) {
                last = Some(l0 + i);
                let replaced = if all {
                    re.replace_all(line, replacement.as_str())
                } else {
                    re.replacen(line, 1, replacement.as_str())
                };
                out.push_str(&replaced);
            } else {
                out.push_str(line);
            }
        }
        let last = last.ok_or_else(|| format!("見つかりません: {pat}"))?;
        // 置き換えで行が増えることがある（\n）ので、最後に置き換えた行は、増えた行数を足して求める
        let grown = out.matches('\n').count() - t[start..end].matches('\n').count();
        t.replace_range(start..end, &out);
        *cursor = first_nonblank(t, nth_line(t, last + grown));
        Ok(())
    }
}

/// `delim` で区切る（`\` + `delim` は区切りにしない）
fn split_unescaped(s: &str, delim: char) -> Vec<String> {
    let mut parts = vec![String::new()];
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some(d) if d == delim => parts.last_mut().expect("一つはある").push(d),
                Some(d) => {
                    let p = parts.last_mut().expect("一つはある");
                    p.push('\\');
                    p.push(d);
                }
                None => parts.last_mut().expect("一つはある").push('\\'),
            }
        } else if c == delim {
            parts.push(String::new());
        } else {
            parts.last_mut().expect("一つはある").push(c);
        }
    }
    parts
}

/// Vim の置き換えの文字列を、regex クレートの置き換えの書き方にする
fn vim_replacement(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match c {
            '&' => out.push_str("${0}"),
            '$' => out.push_str("$$"),
            '\\' => match chars.next() {
                Some(d @ '0'..='9') => out.push_str(&format!("${{{d}}}")),
                Some('n' | 'r') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('&') => out.push('&'),
                Some(d) => out.push(d),
                None => out.push('\\'),
            },
            c => out.push(c),
        }
    }
    out
}

fn toggle_case(s: &str) -> String {
    s.chars()
        .flat_map(|c| {
            if c.is_lowercase() {
                c.to_uppercase().collect::<Vec<_>>()
            } else {
                c.to_lowercase().collect::<Vec<_>>()
            }
        })
        .collect()
}

/// 全角英数字や記号を半角にする（IMEがオンのままでもノーマルモードのキーとして使えるように）。
pub fn normalize_key(c: char) -> char {
    match c {
        '\u{FF01}'..='\u{FF5E}' => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
        '\u{3000}' => ' ',
        '。' => '.',
        '、' => ',',
        '・' => '/',
        'ー' => '-',
        '「' => '[',
        '」' => ']',
        _ => c,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `|` をカーソルにしたテキストに、キー列（`<esc>` `<cr>` `<bs>` `<c-r>` が使える）を送る。
    fn run(marked: &str, keys: &str) -> (String, Vim) {
        let mut vim = Vim::default();
        let (out, _) = feed(&mut vim, marked, keys);
        (out, vim)
    }

    fn feed(vim: &mut Vim, marked: &str, keys: &str) -> (String, Vec<Effects>) {
        let mut cursor = marked.find('|').expect("cursor");
        let mut text = marked.replacen('|', "", 1);
        let mut all = Vec::new();
        let mut rest = keys;
        while !rest.is_empty() {
            let (key, len) = if let Some(r) = rest.strip_prefix("<esc>") {
                let _ = r;
                (Key::Esc, 5)
            } else if rest.starts_with("<cr>") {
                (Key::Enter, 4)
            } else if rest.starts_with("<bs>") {
                (Key::Backspace, 4)
            } else if rest.starts_with("<c-r>") {
                (Key::CtrlR, 5)
            } else if rest.starts_with("<c-o>") {
                (Key::CtrlO, 5)
            } else if rest.starts_with("<c-i>") {
                (Key::CtrlI, 5)
            } else if rest.starts_with("<c-v>") {
                (Key::CtrlV, 5)
            } else {
                let c = rest.chars().next().unwrap();
                (Key::Char(c), c.len_utf8())
            };
            rest = &rest[len..];
            // 挿入モードの文字は、実際にはエディタが入れる（ここで真似する）
            if vim.mode == Mode::Insert && vim.cmdline.is_none() {
                if let Key::Char(c) = key {
                    text.insert(cursor, c);
                    cursor += c.len_utf8();
                    continue;
                }
                if key == Key::Enter {
                    text.insert(cursor, '\n');
                    cursor += 1;
                    continue;
                }
            }
            all.push(vim.handle(&mut text, &mut cursor, key));
        }
        (format!("{}|{}", &text[..cursor], &text[cursor..]), all)
    }

    fn t(marked: &str, keys: &str) -> String {
        run(marked, keys).0
    }

    #[test]
    fn motions() {
        assert_eq!(t("|hello world", "w"), "hello |world");
        assert_eq!(t("|hello world", "e"), "hell|o world");
        assert_eq!(t("hello |world", "b"), "|hello world");
        assert_eq!(t("|a.b c", "w"), "a|.b c");
        assert_eq!(t("|a.b c", "W"), "a.b |c");
        assert_eq!(t("|one two three", "2w"), "one two |three");
        assert_eq!(t("ab|c\nd", "l"), "ab|c\nd");
        assert_eq!(t("a|bc", "$"), "ab|c");
        assert_eq!(t("  a|bc", "0"), "|  abc");
        assert_eq!(t("  a|bc", "^"), "  |abc");
        assert_eq!(t("abc\nd|e\nfgh", "gg"), "|abc\nde\nfgh");
        assert_eq!(t("a|bc\nde\nfgh", "G"), "abc\nde\n|fgh");
        assert_eq!(t("a|bc\nde\nfgh", "2G"), "abc\n|de\nfgh");
        assert_eq!(t("|a,b,c", "f,;"), "a,b|,c");
        assert_eq!(t("|a,b,c", "t,"), "|a,b,c");
        assert_eq!(t("|ab,c", "t,"), "a|b,c");
        assert_eq!(t("|a,b,c", "t,;"), "a,|b,c");
        assert_eq!(t("a,b,|c", "F,"), "a,b|,c");
        assert_eq!(t("|f(a[b]c)", "%"), "f(a[b]c|)");
        // 上下移動は列を覚えておく
        assert_eq!(t("abc|d\nx\nabcd", "jj"), "abcd\nx\nabc|d");
        assert_eq!(t("|p1\np1\n\np2", "}"), "p1\np1\n|\np2");
        // 日本語は文字の種類で区切る
        assert_eq!(t("|日本語のテキスト", "w"), "日本語|のテキスト");
        assert_eq!(t("|日本語のテキスト", "ww"), "日本語の|テキスト");
        assert_eq!(t("abc\n|\ndef", "k"), "|abc\n\ndef");
    }

    #[test]
    fn operators() {
        assert_eq!(t("|hello world", "dw"), "|world");
        assert_eq!(t("hello |world\nnext", "dw"), "hello| \nnext");
        assert_eq!(t("|hello world", "cwhi<esc>"), "h|i world");
        assert_eq!(t("|a b c d", "d2w"), "|c d");
        assert_eq!(t("|a b c d", "2dw"), "|c d");
        assert_eq!(t("a|bc def", "d$"), "|a");
        assert_eq!(t("a|bc def", "D"), "|a");
        assert_eq!(t("one\n|two\nthree", "dd"), "one\n|three");
        assert_eq!(t("one\ntwo\n|three", "dd"), "one\n|two");
        assert_eq!(t("|one\ntwo\nthree", "2dd"), "|three");
        assert_eq!(t("|one\ntwo", "dj"), "|");
        assert_eq!(t("  |one\ntwo", "cc"), "  |\ntwo");
        assert_eq!(t("f(a, |b)", "di("), "f(|)");
        assert_eq!(t("f(a, |b)", "da("), "|f");
        assert_eq!(t("say \"hi |there\" ok", "ci\"yo<esc>"), "say \"y|o\" ok");
        assert_eq!(t("foo b|ar baz", "diw"), "foo | baz");
        assert_eq!(t("foo b|ar baz", "daw"), "foo |baz");
        assert_eq!(t("p1\n|p1\n\np2", "dap"), "|p2");
        assert_eq!(t("|a\nb", ">>"), "\t|a\nb");
        assert_eq!(t("\t|a", "<<"), "|a");
        assert_eq!(t("|hello", "gUiw"), "|HELLO");
        assert_eq!(t("|Hello", "g~~"), "|hELLO");
    }

    #[test]
    fn edits() {
        assert_eq!(t("a|bc", "x"), "a|c");
        assert_eq!(t("ab|c", "x"), "a|b");
        assert_eq!(t("a|bcd", "2x"), "a|d");
        assert_eq!(t("ab|c", "X"), "a|c");
        assert_eq!(t("a|bc", "rx"), "a|xc");
        assert_eq!(t("|abc", "~~"), "AB|c");
        assert_eq!(t("|a\n  b", "J"), "a| b");
        assert_eq!(t("|abc", "ix<esc>"), "|xabc");
        assert_eq!(t("|abc", "ax<esc>"), "a|xbc");
        assert_eq!(t("a|bc", "Ax<esc>"), "abc|x");
        assert_eq!(t("  a|bc", "Ix<esc>"), "  |xabc");
        assert_eq!(t("  |a", "ob<esc>"), "  a\n  |b");
        assert_eq!(t("|a", "Ob<esc>"), "|b\na");
        assert_eq!(t("a|bc", "sX<esc>"), "a|Xc");
        assert_eq!(t("a|bc", "Cz<esc>"), "a|z");
    }

    #[test]
    fn yank_put_and_repeat() {
        assert_eq!(t("|ab cd", "ywP"), "ab| ab cd");
        assert_eq!(t("|one\ntwo", "yyp"), "one\n|one\ntwo");
        assert_eq!(t("|one\ntwo", "ddp"), "two\n|one");
        assert_eq!(t("a|bc", "xp"), "ac|b");
        assert_eq!(t("|a b c d", "dw.."), "|d");
        assert_eq!(t("|x x x", "cwy<esc>w.w."), "y y |y");
        assert_eq!(t("|a\nb\nc", "A;<esc>j.j."), "a;\nb;\nc|;");
        let (_, fx) = feed(&mut Vim::default(), "|ab", "yl");
        assert_eq!(fx.last().unwrap().yank.as_deref(), Some("a"));
    }

    #[test]
    fn visual() {
        assert_eq!(t("|abc def", "vld"), "|c def");
        assert_eq!(t("|abc def", "vey"), "|abc def");
        assert_eq!(t("a\n|b\nc\nd", "Vjd"), "a\n|d");
        assert_eq!(t("|ab", "vU").replace('|', ""), "Ab");
        let (_, vim) = run("|abc", "vl");
        assert_eq!(vim.mode, Mode::Visual);
        assert_eq!(vim.selection("abc", 1), Some((0, 2)));
        let (_, vim) = run("|abc", "vl<esc>");
        assert_eq!(vim.mode, Mode::Normal);
    }

    #[test]
    fn registers() {
        // 名前付きのレジスタは、無名のレジスタを上書きしても残る
        assert_eq!(t("|one two", "\"ayiwwdiw\"ap"), "one on|e");
        // 大文字は追加
        assert_eq!(t("|ab cd", "\"ayl\"Aylw\"ap"), "ab ca|ad");
        // 行単位の追加
        assert_eq!(t("|a\nb", "\"ayyj\"Ayy\"ap"), "a\nb\n|a\nb");
        // "_ は捨てる（無名のレジスタは変わらない）
        assert_eq!(t("|ab cd", "yiww\"_diwP"), "aba|b ");
        // "0 は最後のヤンク（消しても変わらない）
        assert_eq!(t("|ab cd", "yiwwdiw\"0P"), "aba|b ");
        // 消したものは無名のレジスタに入る
        assert_eq!(t("|ab cd", "yiwwdiwP"), "abc|d ");
    }

    #[test]
    fn macros() {
        // 記録して再生する（挿入した文字も記録する）
        assert_eq!(t("|a\nb\nc", "qqA;<esc>jq@q@@"), "a;\nb;\nc|;");
        assert_eq!(t("|1 2 3 4", "qwxlq2@w"), "   |4");
        let (_, vim) = run("|abc", "qa");
        assert_eq!(vim.recording(), Some('a'));
        let (_, vim) = run("|abc", "qaxq");
        assert_eq!(vim.recording(), None);
        // 記録の無いレジスタは、その文字列をキーとして使う
        assert_eq!(t("|dw\nabc def", "\"ay$j@a"), "dw\n|def");
    }

    #[test]
    fn marks_and_jumps() {
        assert_eq!(t("a|bc\nd\nef", "majj`a"), "a|bc\nd\nef");
        assert_eq!(t("  a|bc\nd\nef", "majj'a"), "  |abc\nd\nef");
        assert_eq!(t("a|bc\nd\nef", "mbjjd'b"), "|");
        // '' は最後のジャンプの前へ戻る
        assert_eq!(t("a|b\nc\nd", "G''"), "|ab\nc\nd");
        // ジャンプリスト
        assert_eq!(t("|a\nb\nc", "G<c-o>"), "|a\nb\nc");
        assert_eq!(t("|a\nb\nc", "G<c-o><c-i>"), "a\nb\n|c");
        assert_eq!(t("|a\nb\nc\nd", "2GG<c-o><c-o>"), "|a\nb\nc\nd");
        // 上で行を足すと、マークも下へずれる
        assert_eq!(t("a\n|b", "maggOx<esc>`a"), "x\na\n|b");
    }

    #[test]
    fn visual_block() {
        assert_eq!(t("|abc\ndef\nghi", "<c-v>jld"), "|c\nf\nghi");
        assert_eq!(t("|abc\ndef", "<c-v>jIX<esc>"), "|Xabc\nXdef");
        assert_eq!(t("|ab\nc\nde", "<c-v>jjlA!<esc>"), "ab|!\nc !\nde!");
        assert_eq!(t("|abc\ndef", "<c-v>jlrx"), "|xxc\nxxf");
        assert_eq!(t("|abc\ndef", "<c-v>jlc-<esc>"), "|-c\n-f");
        assert_eq!(t("|abc\ndef", "<c-v>jUl").replace('|', ""), "Abc\nDef");
        // 矩形のヤンクと貼り付け
        assert_eq!(t("|ab\ncd\n", "<c-v>jy$p"), "ab|a\ncdc\n");
        let (_, vim) = run("|abc\ndef", "<c-v>jl");
        assert_eq!(vim.mode, Mode::VisualBlock);
        assert_eq!(vim.block_ranges("abc\ndef", 5), vec![(0, 2), (4, 6)]);
    }

    #[test]
    fn substitute() {
        assert_eq!(t("|foo foo\nfoo", ":s/foo/bar/<cr>"), "|bar foo\nfoo");
        assert_eq!(t("|foo foo\nfoo", ":s/foo/bar/g<cr>"), "|bar bar\nfoo");
        assert_eq!(t("|foo\nfoo\nfoo", ":%s/foo/x/<cr>"), "x\nx\n|x");
        assert_eq!(t("|a\nb\nc\nd", ":2,3s/$/;/<cr>"), "a\nb;\n|c;\nd");
        assert_eq!(t("|Foo", ":s/foo/x/i<cr>"), "|x");
        // & と \1
        assert_eq!(t("|ab12", ":s/(\\d+)/[\\1&]/<cr>"), "|ab[1212]");
        // 区切りを変える・区切りを \ で入れる
        assert_eq!(t("|a/b", ":s#/#-#<cr>"), "|a-b");
        assert_eq!(t("|a/b", ":s/\\//-/<cr>"), "|a-b");
        // ビジュアルモードの範囲
        assert_eq!(t("a\n|b\nc\nd", "Vj:s/^/> /<cr>"), "a\n> b\n|> c\nd");
        // 見つからなければ知らせる
        let (_, fx) = feed(&mut Vim::default(), "|abc", ":s/z/y/<cr>");
        assert!(matches!(fx.last().unwrap().cmd, Some(Cmd::Message(_))));
    }

    #[test]
    fn commands() {
        let cmd = |keys: &str| {
            let (_, fx) = feed(&mut Vim::default(), "|ab cd\nef", keys);
            fx.into_iter().filter_map(|f| f.cmd).collect::<Vec<_>>()
        };
        assert_eq!(cmd(":w<cr>"), [Cmd::Save]);
        assert_eq!(cmd(":wq<cr>"), [Cmd::SaveClose]);
        assert_eq!(cmd(":q<cr>"), [Cmd::Close]);
        assert_eq!(cmd(":foo<cr>"), [Cmd::Unknown("foo".into())]);
        assert_eq!(cmd(":x<bs><esc>"), []);
        assert_eq!(cmd("u"), [Cmd::Undo(1)]);
        assert_eq!(cmd("3u"), [Cmd::Undo(3)]);
        assert_eq!(cmd("<c-r>"), [Cmd::Redo(1)]);
        assert_eq!(cmd("/"), [Cmd::OpenFind(true)]);
        assert_eq!(cmd("n"), [Cmd::FindNext]);
        assert_eq!(cmd("*"), [Cmd::SearchWord("ab".into(), true)]);
        assert_eq!(t("|a\nb\nc", ":3<cr>"), "a\nb\n|c");
        // 不完全なキー列は保留、無効なキー列は捨てる
        let (_, vim) = run("|abc", "d");
        assert_eq!(vim.pending(), "d");
        let (_, vim) = run("|abc", "dz");
        assert_eq!(vim.pending(), "");
        assert_eq!(normalize_key('ｊ'), 'j');
        assert_eq!(normalize_key('：'), ':');
    }
}
