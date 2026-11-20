//! Helix風のモーダル編集（「選択してから操作する」）。
//!
//! Vim のエンジン（[`super::Vim`]）と、モード・選択の起点（`anchor`）・レジスタ・`:` コマンドを共有する。
//! 選択は「起点」と「頭」（カーソル）の2つの文字で表し、両端の文字を含む。
//! ノーマルモードでは移動のたびに選択を作り直し、`v`（SELECT）では起点を残したまま広げる。

use super::*;

impl Vim {
    /// 選択の範囲（バイト位置、終わりは含まない）
    fn hx_range(&self, t: &str, head: usize) -> (usize, usize) {
        let (a, b) = (self.anchor.min(head), self.anchor.max(head));
        (a, next(t, b).min(t.len()).max(b))
    }

    /// 移動の結果を選択にする。`from` は新しい選択の起点（拡張モードでは起点を変えない）。
    fn hx_select(&mut self, from: usize, to: usize, head: &mut usize, t: &str) {
        if self.mode != Mode::Visual {
            self.anchor = from.min(t.len());
        }
        *head = to.min(t.len());
    }

    /// Helix のキーを1つ処理する（挿入モード以外）。
    pub(super) fn helix_key(&mut self, t: &mut String, cursor: &mut usize, key: Key) -> Effects {
        let mut fx = Effects::default();
        let p = *cursor;
        let extend = self.mode == Mode::Visual;
        match key {
            Key::Esc => {
                self.keys.clear();
                if extend {
                    self.mode = Mode::Normal;
                }
                self.anchor = p;
                return fx;
            }
            Key::CtrlR => {
                self.keys.clear();
                fx.cmd = Some(Cmd::Redo(1));
                return fx;
            }
            Key::CtrlD | Key::CtrlU | Key::Enter | Key::Backspace => {
                self.keys.clear();
                let m = match key {
                    Key::CtrlD => Motion::HalfDown,
                    Key::CtrlU => Motion::HalfUp,
                    Key::Enter => Motion::Down,
                    _ => Motion::Left,
                };
                if let Some((to, _)) = self.motion_target(&m, 1, t, p) {
                    self.hx_select(to, to, cursor, t);
                }
                return fx;
            }
            // ジャンプリストと矩形選択は Vim 風のときだけ
            Key::CtrlO | Key::CtrlI | Key::CtrlV => {
                self.keys.clear();
                return fx;
            }
            Key::Char(_) => self.keys.push(key),
        }
        let chars: Vec<char> = self.pending().chars().collect();
        let digits = chars
            .iter()
            .enumerate()
            .take_while(|(i, c)| c.is_ascii_digit() && (*i > 0 || **c != '0'))
            .count();
        let n: usize = chars[..digits]
            .iter()
            .collect::<String>()
            .parse()
            .unwrap_or(1);
        let body = &chars[digits..];
        let Some(&c) = body.first() else {
            return fx;
        };
        // 2文字のコマンドは、2文字目を待つ
        if matches!(c, 'g' | 'f' | 't' | 'F' | 'T' | 'r') && body.len() < 2 {
            return fx;
        }
        self.keys.clear();
        let (a, b) = self.hx_range(t, p);
        // 選択を作る移動の起点: すでに選択していれば、その続きから
        let start = if self.anchor != p && !extend && p < t.len() {
            next(t, p).min(t.len())
        } else {
            p
        };
        match c {
            // ---- 移動（選択を作り直す） ----
            'h' | 'j' | 'k' | 'l' => {
                let m = match c {
                    'h' => Motion::Left,
                    'j' => Motion::Down,
                    'k' => Motion::Up,
                    _ => Motion::Right,
                };
                if let Some((to, _)) = self.motion_target(&m, n, t, p) {
                    let to = if extend { to } else { clamp_normal(t, to) };
                    self.hx_select(to, to, cursor, t);
                }
            }
            'w' | 'W' => {
                let big = c == 'W';
                let mut from = start;
                let mut to = from;
                for i in 0..n {
                    if i > 0 {
                        from = next(t, to).min(t.len());
                    }
                    let s = word_fwd(t, from, big);
                    to = if s > from { prev(t, s) } else { from };
                }
                self.hx_select(if n > 1 { start } else { from }, to, cursor, t);
            }
            'e' | 'E' => {
                let big = c == 'E';
                let mut to = start;
                for _ in 0..n {
                    to = word_end(t, to, big);
                }
                self.hx_select(start, to, cursor, t);
            }
            'b' | 'B' => {
                let big = c == 'B';
                let from = if self.anchor != p && !extend && p > 0 {
                    prev(t, p)
                } else {
                    p
                };
                let mut to = from;
                for _ in 0..n {
                    to = word_back(t, to, big);
                }
                self.hx_select(from, to, cursor, t);
            }
            'x' => {
                // 行全体を選ぶ。すでに行全体を選んでいれば、次の行まで広げる
                let ls = line_start(t, a);
                let whole = a == ls && b > a && (t[..b].ends_with('\n') || b == t.len());
                let mut end = if whole { b } else { line_start(t, p) };
                for _ in 0..n {
                    let le = line_end(t, end);
                    end = if le < t.len() { le + 1 } else { le };
                }
                self.anchor = if whole { a } else { line_start(t, p) };
                *cursor = prev(t, end).max(self.anchor);
            }
            '%' => {
                self.anchor = 0;
                *cursor = prev(t, t.len());
            }
            ';' => self.anchor = p,
            'v' => {
                self.mode = if extend { Mode::Normal } else { Mode::Visual };
            }
            'g' => {
                let to = match body[1] {
                    'g' => Some(first_nonblank(t, 0)),
                    'e' => Some(line_start(t, t.len())),
                    'h' => Some(line_start(t, p)),
                    'l' => Some(clamp_normal(t, line_end(t, p))),
                    's' => Some(first_nonblank(t, p)),
                    _ => None,
                };
                if let Some(to) = to {
                    self.hx_select(to, to, cursor, t);
                }
            }
            'f' | 't' | 'F' | 'T' => {
                let fwd = c.is_lowercase();
                let till = c.eq_ignore_ascii_case(&'t');
                self.last_find = Some((body[1], fwd, till));
                if let Some(to) = find_in_line(t, p, body[1], fwd, till, n, false) {
                    self.hx_select(p, to, cursor, t);
                }
            }
            // ---- 選択への操作 ----
            'd' | 'c' => {
                self.yank_text(&t[a..b], false, false, &mut fx);
                t.replace_range(a..b, "");
                fx.changed = b > a;
                self.mode = Mode::Normal;
                if c == 'c' {
                    *cursor = a;
                    self.enter_insert(t, a);
                } else {
                    *cursor = clamp_normal(t, a);
                }
                self.anchor = *cursor;
            }
            'y' => self.yank_text(&t[a..b], false, true, &mut fx),
            'p' | 'P' => {
                let text = self.register.text.clone();
                if !text.is_empty() {
                    let at = if c == 'p' { b } else { a };
                    t.insert_str(at, &text);
                    fx.changed = true;
                    // 貼り付けた文字列を選ぶ
                    self.anchor = at;
                    *cursor = prev(t, at + text.len()).max(at);
                }
            }
            'r' => {
                let r = body[1];
                let replaced: String = t[a..b]
                    .chars()
                    .map(|ch| if ch == '\n' { ch } else { r })
                    .collect();
                if replaced != t[a..b] {
                    let head_is_end = p >= self.anchor;
                    t.replace_range(a..b, &replaced);
                    fx.changed = true;
                    let end = prev(t, a + replaced.len()).max(a);
                    (self.anchor, *cursor) = if head_is_end { (a, end) } else { (end, a) };
                }
            }
            '~' | '`' => {
                let src = t[a..b].to_string();
                let out = if c == '~' {
                    toggle_case(&src)
                } else {
                    src.to_lowercase()
                };
                if out != src {
                    t.replace_range(a..b, &out);
                    fx.changed = true;
                }
            }
            '>' | '<' => {
                let op = if c == '>' { Op::Indent } else { Op::Outdent };
                let keep = self.mode;
                self.apply(op, a, prev(t, b).max(a), true, t, cursor, &mut fx);
                self.mode = keep;
                self.anchor = *cursor;
            }
            'J' => {
                // 選択した行（1行だけなら次の行も）をつなぐ
                let lines = line_no(t, prev(t, b).max(a)) - line_no(t, a);
                *cursor = a;
                self.run(Action::Join, lines.max(1), t, cursor, &mut fx);
                self.anchor = *cursor;
            }
            // ---- 挿入 ----
            'i' | 'a' | 'I' | 'A' | 'o' | 'O' => {
                self.mode = Mode::Normal;
                let at = match c {
                    'i' => a,
                    'a' => b,
                    _ => {
                        *cursor = p;
                        self.run(Action::Insert(c), 1, t, cursor, &mut fx);
                        self.anchor = *cursor;
                        return fx;
                    }
                };
                *cursor = at;
                self.anchor = at;
                self.enter_insert(t, at);
            }
            'u' => fx.cmd = Some(Cmd::Undo(n)),
            'U' => fx.cmd = Some(Cmd::Redo(n)),
            '/' | '?' => fx.cmd = Some(Cmd::OpenFind(c == '/')),
            'n' => fx.cmd = Some(Cmd::FindNext),
            'N' => fx.cmd = Some(Cmd::FindPrev),
            '*' => {
                let w = t[a..b].trim();
                if !w.is_empty() {
                    fx.cmd = Some(Cmd::SearchWord(w.to_string(), true));
                }
            }
            ':' => self.cmdline = Some(String::new()),
            _ => {}
        }
        fx
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `[` を起点、`|` を頭（カーソル）としたテキストにキーを送り、結果を同じ書き方で返す。
    /// 起点と頭が同じなら `|` だけを書く。
    fn hx(marked: &str, keys: &str) -> String {
        let mut vim = Vim {
            helix: true,
            ..Vim::default()
        };
        let mut cursor = marked.find('|').expect("cursor");
        let mut text = marked.replacen('|', "", 1);
        vim.anchor = cursor;
        let mut rest = keys;
        while !rest.is_empty() {
            let (key, len) = if rest.starts_with("<esc>") {
                (Key::Esc, 5)
            } else {
                let c = rest.chars().next().unwrap();
                (Key::Char(c), c.len_utf8())
            };
            rest = &rest[len..];
            if vim.mode == Mode::Insert && vim.cmdline.is_none() {
                if let Key::Char(c) = key {
                    text.insert(cursor, c);
                    cursor += c.len_utf8();
                    continue;
                }
            }
            vim.handle(&mut text, &mut cursor, key);
        }
        if vim.anchor == cursor || vim.mode == Mode::Insert {
            return format!("{}|{}", &text[..cursor], &text[cursor..]);
        }
        let mut out = text.clone();
        let (first, second) = if vim.anchor < cursor {
            ((cursor, '|'), (vim.anchor, '['))
        } else {
            ((vim.anchor, '['), (cursor, '|'))
        };
        out.insert(first.0, first.1);
        out.insert(second.0, second.1);
        out
    }

    #[test]
    fn select_then_act() {
        // w は次の単語の手前まで（後ろの空白を含む）を選ぶ
        assert_eq!(hx("|foo bar baz", "w"), "[foo| bar baz");
        assert_eq!(hx("|foo bar baz", "ww"), "foo [bar| baz");
        assert_eq!(hx("|foo bar baz", "wd"), "|bar baz");
        assert_eq!(hx("|foo bar baz", "e"), "[fo|o bar baz");
        assert_eq!(hx("foo ba|r", "b"), "foo |ba[r");
        // c は消して挿入モード
        assert_eq!(hx("|foo bar", "ecX<esc>"), "X| bar");
        // x は行全体、もう一度で次の行まで
        assert_eq!(hx("a|b\ncd\nef", "xd"), "|cd\nef");
        assert_eq!(hx("a|b\ncd\nef", "xxd"), "|ef");
        // % で全体、y と p
        assert_eq!(hx("|ab", "%y<esc>p"), "ab[a|b");
        // f で文字まで選ぶ、r で置き換え
        assert_eq!(hx("|hello world", "fo"), "[hell|o world");
        assert_eq!(hx("|hello world", "fod"), "| world");
        assert_eq!(hx("|abc def", "erx"), "[xx|x def");
        // ~ と `
        assert_eq!(hx("|Abc def", "e~"), "[aB|C def");
        // v で広げる
        assert_eq!(hx("|one two three", "vee"), "[one tw|o three");
        // ; で選択を畳む
        assert_eq!(hx("|one two", "w;"), "one| two");
        // i / a は選択の前 / 後ろに入る
        assert_eq!(hx("|ab cd", "eiX<esc>"), "X|ab cd");
        assert_eq!(hx("|ab cd", "eaX<esc>"), "abX| cd");
        // h j k l は選択を作らない
        assert_eq!(hx("|abc\ndef", "jl"), "abc\nd|ef");
        // 数値の前置
        assert_eq!(hx("|a b c d", "2w"), "[a b| c d");
    }
}
