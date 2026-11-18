//! 数式（TeXの数式モードのよく使う部分）を木に解析する。描画はGUI側で行う。
//!
//! 対応: 文字と記号（ギリシャ文字・演算子・矢印・集合など）、`^` `_`、`\frac` `\sqrt`、
//! `\sum` などの大型演算子、`\sin` などの関数名、`\text`、`\mathbb`、アクセント（`\hat` `\bar` `\vec` …）、
//! `\left( … \right)`、`matrix` / `pmatrix` / `bmatrix` / `vmatrix` / `cases` / `aligned` 環境。
//! 知らない命令はそのまま `\命令` と表示する（描けないときに黙って消さない）。
//!
//! 文字列だけで表したいとき（HTML書き出しなど）は [`to_unicode`] を使う。

/// 数式の要素
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    /// 文字・記号（変数は斜体で描く）
    Text {
        text: String,
        italic: bool,
    },
    /// 関数名・`\text` など、立体で描く文字列
    Upright(String),
    Row(Vec<Node>),
    Frac {
        num: Box<Node>,
        den: Box<Node>,
        /// 分数線を引くか（`\binom` では引かない）
        line: bool,
    },
    Sqrt {
        index: Option<Box<Node>>,
        body: Box<Node>,
    },
    Scripts {
        base: Box<Node>,
        sub: Option<Box<Node>>,
        sup: Option<Box<Node>>,
    },
    /// `\sum` `\int` など。表示数式では添字を上下に置く（`limits`）
    BigOp {
        symbol: String,
        limits: bool,
    },
    /// 上（`\underline` は下）に付ける記号
    Accent {
        mark: Accent,
        body: Box<Node>,
    },
    /// `\left( … \right)`: 中身の高さに合わせて伸ばす括弧（空文字列は省略）
    Delim {
        open: String,
        close: String,
        body: Box<Node>,
    },
    /// 行列などの表。括弧は `open` / `close`（空なら無し）
    Matrix {
        rows: Vec<Vec<Node>>,
        open: String,
        close: String,
        /// `aligned` のように、列を右・左交互にそろえる
        align_pairs: bool,
    },
    /// 空白（全角の何倍か）
    Space(f32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Accent {
    Hat,
    Tilde,
    Dot,
    DDot,
    Vec,
    Bar,
    Underline,
}

impl Accent {
    /// 文字の後ろに付ける結合文字（文字列にするとき）
    fn combining(self) -> char {
        match self {
            Accent::Hat => '\u{302}',
            Accent::Tilde => '\u{303}',
            Accent::Dot => '\u{307}',
            Accent::DDot => '\u{308}',
            Accent::Vec => '\u{20D7}',
            Accent::Bar => '\u{305}',
            Accent::Underline => '\u{332}',
        }
    }
}

fn symbol(name: &str) -> Option<&'static str> {
    Some(match name {
        // ギリシャ文字
        "alpha" => "α",
        "beta" => "β",
        "gamma" => "γ",
        "delta" => "δ",
        "epsilon" => "ϵ",
        "varepsilon" => "ε",
        "zeta" => "ζ",
        "eta" => "η",
        "theta" => "θ",
        "vartheta" => "ϑ",
        "iota" => "ι",
        "kappa" => "κ",
        "lambda" => "λ",
        "mu" => "μ",
        "nu" => "ν",
        "xi" => "ξ",
        "pi" => "π",
        "varpi" => "ϖ",
        "rho" => "ρ",
        "varrho" => "ϱ",
        "sigma" => "σ",
        "varsigma" => "ς",
        "tau" => "τ",
        "upsilon" => "υ",
        "phi" => "ϕ",
        "varphi" => "φ",
        "chi" => "χ",
        "psi" => "ψ",
        "omega" => "ω",
        "Gamma" => "Γ",
        "Delta" => "Δ",
        "Theta" => "Θ",
        "Lambda" => "Λ",
        "Xi" => "Ξ",
        "Pi" => "Π",
        "Sigma" => "Σ",
        "Upsilon" => "Υ",
        "Phi" => "Φ",
        "Psi" => "Ψ",
        "Omega" => "Ω",
        // 演算子・関係
        "times" => "×",
        "div" => "÷",
        "cdot" => "⋅",
        "ast" => "∗",
        "star" => "⋆",
        "circ" => "∘",
        "bullet" => "∙",
        "pm" => "±",
        "mp" => "∓",
        "leq" | "le" => "≤",
        "geq" | "ge" => "≥",
        "neq" | "ne" => "≠",
        "approx" => "≈",
        "equiv" => "≡",
        "sim" => "∼",
        "simeq" => "≃",
        "cong" => "≅",
        "propto" => "∝",
        "ll" => "≪",
        "gg" => "≫",
        "prec" => "≺",
        "succ" => "≻",
        "perp" => "⊥",
        "parallel" => "∥",
        "mid" => "∣",
        // 集合・論理
        "in" => "∈",
        "notin" => "∉",
        "ni" => "∋",
        "subset" => "⊂",
        "supset" => "⊃",
        "subseteq" => "⊆",
        "supseteq" => "⊇",
        "cup" => "∪",
        "cap" => "∩",
        "setminus" => "∖",
        "emptyset" | "varnothing" => "∅",
        "forall" => "∀",
        "exists" => "∃",
        "nexists" => "∄",
        "neg" | "lnot" => "¬",
        "land" | "wedge" => "∧",
        "lor" | "vee" => "∨",
        "oplus" => "⊕",
        "otimes" => "⊗",
        // 矢印
        "to" | "rightarrow" => "→",
        "leftarrow" | "gets" => "←",
        "leftrightarrow" => "↔",
        "Rightarrow" | "implies" => "⇒",
        "Leftarrow" => "⇐",
        "Leftrightarrow" | "iff" => "⇔",
        "mapsto" => "↦",
        "uparrow" => "↑",
        "downarrow" => "↓",
        "longrightarrow" => "⟶",
        "longleftarrow" => "⟵",
        // その他
        "infty" => "∞",
        "partial" => "∂",
        "nabla" => "∇",
        "hbar" => "ℏ",
        "ell" => "ℓ",
        "Re" => "ℜ",
        "Im" => "ℑ",
        "aleph" => "ℵ",
        "angle" => "∠",
        "triangle" => "△",
        "degree" => "°",
        "prime" => "′",
        "ldots" | "dots" => "…",
        "cdots" => "⋯",
        "vdots" => "⋮",
        "ddots" => "⋱",
        "therefore" => "∴",
        "because" => "∵",
        "langle" => "⟨",
        "rangle" => "⟩",
        "lfloor" => "⌊",
        "rfloor" => "⌋",
        "lceil" => "⌈",
        "rceil" => "⌉",
        "lbrace" | "{" => "{",
        "rbrace" | "}" => "}",
        "vert" => "|",
        "|" => "‖",
        "Vert" => "‖",
        "backslash" => "\\",
        "%" => "%",
        "$" => "$",
        "#" => "#",
        "&" => "&",
        "_" => "_",
        _ => return None,
    })
}

/// 大型演算子: (記号, 表示数式で添字を上下に置くか)
fn big_op(name: &str) -> Option<(&'static str, bool)> {
    Some(match name {
        "sum" => ("∑", true),
        "prod" => ("∏", true),
        "coprod" => ("∐", true),
        "bigcup" => ("⋃", true),
        "bigcap" => ("⋂", true),
        "bigoplus" => ("⨁", true),
        "bigotimes" => ("⨂", true),
        "int" => ("∫", false),
        "iint" => ("∬", false),
        "iiint" => ("∭", false),
        "oint" => ("∮", false),
        _ => return None,
    })
}

/// 立体で書く関数名。`true` は表示数式で添字を下に置くもの（`\lim` など）
fn function(name: &str) -> Option<bool> {
    Some(match name {
        "lim" | "max" | "min" | "sup" | "inf" | "limsup" | "liminf" | "det" | "gcd" | "argmax"
        | "argmin" | "Pr" => true,
        "sin" | "cos" | "tan" | "cot" | "sec" | "csc" | "arcsin" | "arccos" | "arctan" | "sinh"
        | "cosh" | "tanh" | "log" | "ln" | "lg" | "exp" | "deg" | "dim" | "ker" | "hom" | "arg"
        | "mod" | "bmod" => false,
        _ => return None,
    })
}

fn double_struck(c: char) -> char {
    match c {
        'C' => 'ℂ',
        'H' => 'ℍ',
        'N' => 'ℕ',
        'P' => 'ℙ',
        'Q' => 'ℚ',
        'R' => 'ℝ',
        'Z' => 'ℤ',
        'A'..='Z' => char::from_u32(0x1D538 + (c as u32 - 'A' as u32)).unwrap_or(c),
        _ => c,
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Cmd(String),
    Open,
    Close,
    Sup,
    Sub,
    Amp,
    NewRow,
    Char(char),
    Space,
}

fn tokenize(tex: &str) -> Vec<Tok> {
    let chars: Vec<char> = tex.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        i += 1;
        match c {
            '\\' => {
                let Some(&next) = chars.get(i) else {
                    out.push(Tok::Char('\\'));
                    break;
                };
                if next == '\\' {
                    i += 1;
                    out.push(Tok::NewRow);
                } else if next.is_ascii_alphabetic() {
                    let start = i;
                    while i < chars.len() && chars[i].is_ascii_alphabetic() {
                        i += 1;
                    }
                    out.push(Tok::Cmd(chars[start..i].iter().collect()));
                } else {
                    i += 1;
                    out.push(Tok::Cmd(next.to_string()));
                }
            }
            '{' => out.push(Tok::Open),
            '}' => out.push(Tok::Close),
            '^' => out.push(Tok::Sup),
            '_' => out.push(Tok::Sub),
            '&' => out.push(Tok::Amp),
            '\'' => out.push(Tok::Char('′')),
            c if c.is_whitespace() => {
                if out.last() != Some(&Tok::Space) {
                    out.push(Tok::Space);
                }
            }
            c => out.push(Tok::Char(c)),
        }
    }
    out
}

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
    display: bool,
}

/// 並びを読む途中で止まった理由
#[derive(PartialEq)]
enum Stop {
    End,
    Close,
    Amp,
    NewRow,
    Right,
    EndEnv,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).cloned();
        self.pos += 1;
        t
    }

    fn skip_space(&mut self) {
        while self.peek() == Some(&Tok::Space) {
            self.pos += 1;
        }
    }

    /// `{…}` の中身を文字列のまま読む（`\text` や環境名に使う）
    fn raw_group(&mut self) -> String {
        self.skip_space();
        if self.peek() != Some(&Tok::Open) {
            return match self.next() {
                Some(Tok::Char(c)) => c.to_string(),
                _ => String::new(),
            };
        }
        self.pos += 1;
        let mut depth = 0;
        let mut s = String::new();
        while let Some(t) = self.next() {
            match t {
                Tok::Open => {
                    depth += 1;
                    s.push('{');
                }
                Tok::Close if depth == 0 => break,
                Tok::Close => {
                    depth -= 1;
                    s.push('}');
                }
                Tok::Char(c) => s.push(c),
                Tok::Space => s.push(' '),
                Tok::Cmd(c) => match symbol(&c) {
                    Some(sym) => s.push_str(sym),
                    None if c.len() == 1 => s.push_str(&c),
                    None => {
                        s.push('\\');
                        s.push_str(&c);
                    }
                },
                Tok::Sup => s.push('^'),
                Tok::Sub => s.push('_'),
                Tok::Amp => s.push('&'),
                Tok::NewRow => s.push(' '),
            }
        }
        s
    }

    /// 次の1要素（`{…}` なら中身全体）
    fn atom(&mut self) -> Node {
        self.skip_space();
        match self.peek() {
            Some(Tok::Open) => {
                self.pos += 1;
                let (row, _) = self.row();
                row
            }
            Some(_) => self.single().unwrap_or(Node::Row(Vec::new())),
            None => Node::Row(Vec::new()),
        }
    }

    /// 区切りで止まるまで並びを読む
    fn row(&mut self) -> (Node, Stop) {
        let mut items: Vec<Node> = Vec::new();
        loop {
            match self.peek() {
                None => return (Node::Row(items), Stop::End),
                Some(Tok::Close) => {
                    self.pos += 1;
                    return (Node::Row(items), Stop::Close);
                }
                Some(Tok::Amp) => {
                    self.pos += 1;
                    return (Node::Row(items), Stop::Amp);
                }
                Some(Tok::NewRow) => {
                    self.pos += 1;
                    return (Node::Row(items), Stop::NewRow);
                }
                Some(Tok::Cmd(c)) if c == "right" => {
                    self.pos += 1;
                    return (Node::Row(items), Stop::Right);
                }
                Some(Tok::Cmd(c)) if c == "end" => {
                    self.pos += 1;
                    self.raw_group();
                    return (Node::Row(items), Stop::EndEnv);
                }
                Some(Tok::Space) => self.pos += 1,
                Some(Tok::Sup | Tok::Sub) => {
                    let is_sup = self.next() == Some(Tok::Sup);
                    let script = self.atom();
                    let base = items.pop().unwrap_or(Node::Row(Vec::new()));
                    let node = match base {
                        Node::Scripts { base, sub, sup } => {
                            let (sub, sup) = if is_sup {
                                (sub, Some(Box::new(script)))
                            } else {
                                (Some(Box::new(script)), sup)
                            };
                            Node::Scripts { base, sub, sup }
                        }
                        base => Node::Scripts {
                            base: Box::new(base),
                            sub: (!is_sup).then(|| Box::new(script.clone())),
                            sup: is_sup.then(|| Box::new(script)),
                        },
                    };
                    items.push(node);
                }
                Some(_) => {
                    if let Some(n) = self.single() {
                        // `x''` のようなプライムは、直前の要素の上付きとして扱う
                        if matches!(&n, Node::Text { text, .. } if text == "′") {
                            if let Some(last) = items.pop() {
                                items.push(Node::Scripts {
                                    base: Box::new(last),
                                    sub: None,
                                    sup: Some(Box::new(n)),
                                });
                                continue;
                            }
                        }
                        items.push(n);
                    }
                }
            }
        }
    }

    fn delimiter(&mut self) -> String {
        self.skip_space();
        match self.next() {
            Some(Tok::Char('.')) | None => String::new(),
            Some(Tok::Char(c)) => c.to_string(),
            Some(Tok::Cmd(c)) => symbol(&c).map_or_else(|| c.clone(), str::to_string),
            Some(_) => String::new(),
        }
    }

    fn single(&mut self) -> Option<Node> {
        let tok = self.next()?;
        Some(match tok {
            Tok::Char(c) => Node::Text {
                text: c.to_string(),
                italic: c.is_alphabetic(),
            },
            Tok::Open => {
                let (row, _) = self.row();
                row
            }
            Tok::Cmd(cmd) => return self.command(&cmd),
            Tok::Close | Tok::Amp | Tok::NewRow | Tok::Space | Tok::Sup | Tok::Sub => return None,
        })
    }

    fn command(&mut self, cmd: &str) -> Option<Node> {
        Some(match cmd {
            "frac" | "dfrac" | "tfrac" | "cfrac" => {
                let num = self.atom();
                let den = self.atom();
                Node::Frac {
                    num: Box::new(num),
                    den: Box::new(den),
                    line: true,
                }
            }
            "binom" | "dbinom" | "tbinom" => {
                let num = self.atom();
                let den = self.atom();
                Node::Delim {
                    open: "(".into(),
                    close: ")".into(),
                    body: Box::new(Node::Frac {
                        num: Box::new(num),
                        den: Box::new(den),
                        line: false,
                    }),
                }
            }
            "sqrt" => {
                self.skip_space();
                let index = if self.peek() == Some(&Tok::Char('[')) {
                    self.pos += 1;
                    let mut items = Vec::new();
                    while let Some(t) = self.peek() {
                        if *t == Tok::Char(']') {
                            self.pos += 1;
                            break;
                        }
                        if let Some(n) = self.single() {
                            items.push(n);
                        }
                    }
                    Some(Box::new(Node::Row(items)))
                } else {
                    None
                };
                Node::Sqrt {
                    index,
                    body: Box::new(self.atom()),
                }
            }
            "text" | "textrm" | "mathrm" | "operatorname" | "textit" | "textbf" | "mathbf"
            | "mathsf" | "mathtt" | "boldsymbol" | "bm" | "mbox" => {
                let raw = self.raw_group();
                if matches!(cmd, "mathbf" | "boldsymbol" | "bm") {
                    Node::Text {
                        text: raw,
                        italic: false,
                    }
                } else {
                    Node::Upright(raw)
                }
            }
            "mathit" => Node::Text {
                text: self.raw_group(),
                italic: true,
            },
            "mathbb" => Node::Upright(self.raw_group().chars().map(double_struck).collect()),
            "mathcal" | "mathscr" | "mathfrak" => Node::Text {
                text: self.raw_group(),
                italic: true,
            },
            "hat" | "widehat" => self.accent(Accent::Hat),
            "tilde" | "widetilde" => self.accent(Accent::Tilde),
            "dot" => self.accent(Accent::Dot),
            "ddot" => self.accent(Accent::DDot),
            "vec" | "overrightarrow" => self.accent(Accent::Vec),
            "bar" | "overline" => self.accent(Accent::Bar),
            "underline" => self.accent(Accent::Underline),
            "left" => {
                let open = self.delimiter();
                let (body, stop) = self.row();
                let close = if stop == Stop::Right {
                    self.delimiter()
                } else {
                    String::new()
                };
                Node::Delim {
                    open,
                    close,
                    body: Box::new(body),
                }
            }
            "big" | "Big" | "bigg" | "Bigg" | "bigl" | "bigr" | "Bigl" | "Bigr" | "biggl"
            | "biggr" => Node::Text {
                text: self.delimiter(),
                italic: false,
            },
            "begin" => self.environment(),
            "," | "thinspace" => Node::Space(0.17),
            ":" | ">" | "medspace" => Node::Space(0.22),
            ";" | "thickspace" => Node::Space(0.28),
            " " => Node::Space(0.33),
            "quad" => Node::Space(1.0),
            "qquad" => Node::Space(2.0),
            "!" | "displaystyle" | "textstyle" | "limits" | "nolimits" | "label" | "nonumber"
            | "notag" => {
                if cmd == "label" {
                    self.raw_group();
                }
                return None;
            }
            _ => {
                if let Some(sym) = symbol(cmd) {
                    Node::Text {
                        text: sym.to_string(),
                        italic: false,
                    }
                } else if let Some((symbol, limits)) = big_op(cmd) {
                    Node::BigOp {
                        symbol: symbol.to_string(),
                        limits: limits && self.display,
                    }
                } else if let Some(limits) = function(cmd) {
                    let name = if cmd == "bmod" { "mod" } else { cmd };
                    if limits && self.display {
                        Node::BigOp {
                            symbol: name.to_string(),
                            limits: true,
                        }
                    } else {
                        // 関数名と引数の間は少し空ける（\sin x）
                        Node::Row(vec![Node::Upright(name.to_string()), Node::Space(0.17)])
                    }
                } else {
                    // 知らない命令は、書いたとおりに見せる
                    Node::Upright(format!("\\{cmd}"))
                }
            }
        })
    }

    fn accent(&mut self, mark: Accent) -> Node {
        Node::Accent {
            mark,
            body: Box::new(self.atom()),
        }
    }

    fn environment(&mut self) -> Node {
        let name = self.raw_group();
        let name = name.trim_end_matches('*');
        // `array` の列指定（{cc} など）は読み飛ばす
        if name == "array" {
            self.raw_group();
        }
        let mut rows: Vec<Vec<Node>> = vec![Vec::new()];
        loop {
            let (cell, stop) = self.row();
            rows.last_mut().expect("never empty").push(cell);
            match stop {
                Stop::Amp => {}
                Stop::NewRow => rows.push(Vec::new()),
                _ => break,
            }
        }
        // 最後の `\\` の後ろの空行は捨てる
        if rows.len() > 1
            && rows
                .last()
                .is_some_and(|r| r.iter().all(|c| matches!(c, Node::Row(v) if v.is_empty())))
        {
            rows.pop();
        }
        let (open, close) = match name {
            "pmatrix" => ("(", ")"),
            "bmatrix" => ("[", "]"),
            "Bmatrix" => ("{", "}"),
            "vmatrix" => ("|", "|"),
            "Vmatrix" => ("‖", "‖"),
            "cases" => ("{", ""),
            _ => ("", ""),
        };
        Node::Matrix {
            rows,
            open: open.into(),
            close: close.into(),
            align_pairs: matches!(name, "aligned" | "align" | "split" | "gathered" | "alignat"),
        }
    }
}

/// TeXの数式を解析する。`display` は `$$…$$`（表示数式）か。
pub fn parse(tex: &str, display: bool) -> Node {
    let mut p = Parser {
        toks: tokenize(tex),
        pos: 0,
        display,
    };
    let mut items = Vec::new();
    loop {
        let (row, stop) = p.row();
        items.push(row);
        // 対応の取れない `}` や `&` などがあっても、残りを読み続ける
        if stop == Stop::End {
            break;
        }
    }
    if items.len() == 1 {
        items.pop().expect("one item")
    } else {
        Node::Row(items)
    }
}

const SUPERSCRIPTS: &[(char, char)] = &[
    ('0', '⁰'),
    ('1', '¹'),
    ('2', '²'),
    ('3', '³'),
    ('4', '⁴'),
    ('5', '⁵'),
    ('6', '⁶'),
    ('7', '⁷'),
    ('8', '⁸'),
    ('9', '⁹'),
    ('+', '⁺'),
    ('-', '⁻'),
    ('=', '⁼'),
    ('(', '⁽'),
    (')', '⁾'),
    ('n', 'ⁿ'),
    ('i', 'ⁱ'),
    ('′', '′'),
];

const SUBSCRIPTS: &[(char, char)] = &[
    ('0', '₀'),
    ('1', '₁'),
    ('2', '₂'),
    ('3', '₃'),
    ('4', '₄'),
    ('5', '₅'),
    ('6', '₆'),
    ('7', '₇'),
    ('8', '₈'),
    ('9', '₉'),
    ('+', '₊'),
    ('-', '₋'),
    ('=', '₌'),
    ('(', '₍'),
    (')', '₎'),
    ('a', 'ₐ'),
    ('e', 'ₑ'),
    ('i', 'ᵢ'),
    ('j', 'ⱼ'),
    ('k', 'ₖ'),
    ('n', 'ₙ'),
    ('x', 'ₓ'),
];

fn map_all(s: &str, table: &[(char, char)]) -> Option<String> {
    s.chars()
        .map(|c| table.iter().find(|(a, _)| *a == c).map(|(_, b)| *b))
        .collect()
}

fn wrap(s: String) -> String {
    if s.chars().count() <= 1 {
        s
    } else {
        format!("({s})")
    }
}

fn linear(node: &Node, out: &mut String) {
    match node {
        Node::Text { text, .. } | Node::Upright(text) => out.push_str(text),
        Node::Row(items) => items.iter().for_each(|n| linear(n, out)),
        Node::Frac { num, den, .. } => {
            out.push_str(&wrap(to_string(num)));
            out.push('/');
            out.push_str(&wrap(to_string(den)));
        }
        Node::Sqrt { index, body } => {
            if let Some(i) = index {
                out.push_str(&map_all(&to_string(i), SUPERSCRIPTS).unwrap_or_default());
            }
            out.push('√');
            out.push_str(&wrap(to_string(body)));
        }
        Node::Scripts { base, sub, sup } => {
            linear(base, out);
            for (part, table, mark) in [(sub, SUBSCRIPTS, '_'), (sup, SUPERSCRIPTS, '^')] {
                if let Some(p) = part {
                    let s = to_string(p);
                    match map_all(&s, table) {
                        Some(m) => out.push_str(&m),
                        None => {
                            out.push(mark);
                            out.push_str(&wrap(s));
                        }
                    }
                }
            }
        }
        Node::BigOp { symbol, .. } => out.push_str(symbol),
        Node::Accent { mark, body } => {
            let s = to_string(body);
            if s.chars().count() == 1 {
                out.push_str(&s);
                out.push(mark.combining());
            } else {
                out.push_str(&s);
            }
        }
        Node::Delim { open, close, body } => {
            out.push_str(open);
            linear(body, out);
            out.push_str(close);
        }
        Node::Matrix {
            rows, open, close, ..
        } => {
            out.push_str(if open.is_empty() { "[" } else { open });
            let rows: Vec<String> = rows
                .iter()
                .map(|r| r.iter().map(to_string).collect::<Vec<_>>().join(" "))
                .collect();
            out.push_str(&rows.join("; "));
            out.push_str(if close.is_empty() && open.is_empty() {
                "]"
            } else {
                close
            });
        }
        Node::Space(em) => {
            if *em >= 0.2 {
                out.push(' ');
            }
        }
    }
}

fn to_string(node: &Node) -> String {
    let mut s = String::new();
    linear(node, &mut s);
    s
}

/// 1行の文字列で表した数式（上付き・下付きはUnicodeの文字で表せるときだけ使う）。
pub fn to_unicode(tex: &str) -> String {
    to_string(&parse(tex, false)).trim().to_string()
}

/// 行内数式 `$…$` の範囲（`$` を含む、バイト位置）。
/// 金額の `$5 と $10` を数式にしないよう、開きの直後と閉じの直前は空白不可、閉じの直後は数字不可とする
/// （pulldown-cmark・Pandocと同じ考え方）。
pub fn inline_spans(line: &str) -> Vec<std::ops::Range<usize>> {
    let b = line.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' {
            i += 2;
            continue;
        }
        if b[i] != b'$' {
            i += 1;
            continue;
        }
        // `$$` は表示数式の記号なので、行内数式の始まりにはしない
        if b.get(i + 1) == Some(&b'$') {
            i += 2;
            continue;
        }
        let open = i;
        if b.get(open + 1).is_none_or(|c| c.is_ascii_whitespace()) {
            i += 1;
            continue;
        }
        let mut j = open + 1;
        let mut close = None;
        while j < b.len() {
            match b[j] {
                b'\\' => j += 2,
                b'$' => {
                    let ok = !b[j - 1].is_ascii_whitespace()
                        && b.get(j + 1).is_none_or(|c| !c.is_ascii_digit());
                    if ok {
                        close = Some(j);
                    }
                    break;
                }
                _ => j += 1,
            }
        }
        match close {
            Some(c) => {
                out.push(open..c + 1);
                i = c + 1;
            }
            None => i = open + 1,
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_forms() {
        assert_eq!(to_unicode(r"\alpha + \beta^2"), "α+β²");
        assert_eq!(to_unicode(r"x_1, x_{n+1}"), "x₁,xₙ₊₁");
        assert_eq!(to_unicode(r"\frac{a+b}{c}"), "(a+b)/c");
        assert_eq!(to_unicode(r"\sqrt{x}"), "√x");
        assert_eq!(to_unicode(r"\sqrt[3]{x+1}"), "³√(x+1)");
        assert_eq!(to_unicode(r"\sum_{i=1}^{n} i"), "∑ᵢ₌₁ⁿi");
        assert_eq!(to_unicode(r"e^{i\pi}"), "e^(iπ)");
        assert_eq!(to_unicode(r"\mathbb{R}^n"), "ℝⁿ");
        assert_eq!(to_unicode(r"\hat{x}"), "x\u{302}");
        assert_eq!(to_unicode(r"\text{if } x \leq 0"), "if x≤0");
        assert_eq!(to_unicode(r"\left( \frac{1}{2} \right)"), "(1/2)");
        assert_eq!(
            to_unicode(r"\begin{pmatrix} a & b \\ c & d \end{pmatrix}"),
            "(a b; c d)"
        );
        assert_eq!(to_unicode(r"f'(x)"), "f′(x)");
        assert_eq!(to_unicode(r"\foo x"), r"\foox");
        assert_eq!(to_unicode(r"\sin x"), "sinx");
    }

    #[test]
    fn tree_shapes() {
        let n = parse(r"\sum_{i=0}^{n}", true);
        assert!(matches!(
            n,
            Node::Row(ref v) if matches!(&v[0], Node::Scripts { base, sub: Some(_), sup: Some(_) }
                if matches!(**base, Node::BigOp { limits: true, .. }))
        ));
        // 行内では添字を横に付ける
        let n = parse(r"\sum_{i}", false);
        assert!(
            matches!(n, Node::Row(ref v) if matches!(&v[0], Node::Scripts { base, .. } if matches!(**base, Node::BigOp { limits: false, .. })))
        );
        let n = parse(
            r"\begin{cases} 1 & x>0 \\ 0 & \text{otherwise} \\ \end{cases}",
            true,
        );
        let Node::Row(v) = n else { panic!() };
        let Node::Matrix { rows, open, .. } = &v[0] else {
            panic!("{v:?}")
        };
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].len(), 2);
        assert_eq!(open, "{");
        // 壊れた入力でも止まらない
        for bad in [
            r"\frac{",
            r"}}{",
            r"x^",
            r"\left(",
            r"\begin{matrix} a &",
            r"\sqrt[",
            r"\",
        ] {
            let _ = parse(bad, true);
            let _ = to_unicode(bad);
        }
    }

    #[test]
    fn inline_dollar_rules() {
        fn spans(s: &str) -> Vec<&str> {
            inline_spans(s).into_iter().map(|r| &s[r]).collect()
        }
        assert_eq!(spans("式 $x^2$ と $y$。"), ["$x^2$", "$y$"]);
        assert!(spans("価格は $5 と $10 です").is_empty());
        assert!(spans("$ x $").is_empty());
        assert!(spans("$$x$$").is_empty());
        assert_eq!(spans(r"\$ と $a\$b$"), [r"$a\$b$"]);
        assert!(spans("$x$1").is_empty());
    }
}
