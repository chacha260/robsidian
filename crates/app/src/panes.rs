//! ペインの配置: 分割の木と、ペインごとのタブ（GUIに依存しない）。
//!
//! 開いているノート（`App::tabs`）は全体で1つの並びとして持ち、ここでは「どのノートがどのペインに
//! 並んでいるか」と「分割の形」だけを持つ。フォーカスのあるペインは、アクティブなタブのあるペイン。
//! 同じノートを2つのペインに出すことはしない（エディタの状態はノートごとに1つなので）。

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// 分割の向き
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Dir {
    /// 左右に並べる
    Row,
    /// 上下に並べる
    Column,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Node {
    Pane(usize),
    /// `sizes` は子の大きさの割合（合計 1）
    Split {
        dir: Dir,
        children: Vec<Node>,
        sizes: Vec<f32>,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Pane {
    /// このペインのタブ（ノートのパス、並び順）
    pub tabs: Vec<String>,
    /// 表示中のタブ
    pub current: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Panes {
    pub root: Node,
    pub panes: BTreeMap<usize, Pane>,
    pub focused: usize,
}

impl Default for Panes {
    fn default() -> Self {
        Self {
            root: Node::Pane(0),
            panes: BTreeMap::from([(0, Pane::default())]),
            focused: 0,
        }
    }
}

/// 子の大きさの割合の最小値（ドラッグで潰しきらないように）
const MIN_SIZE: f32 = 0.1;

impl Panes {
    pub fn is_split(&self) -> bool {
        self.panes.len() > 1
    }

    /// そのノートのあるペイン
    pub fn pane_of(&self, path: &str) -> Option<usize> {
        self.panes
            .iter()
            .find(|(_, p)| p.tabs.iter().any(|t| t == path))
            .map(|(&id, _)| id)
    }

    pub fn tabs(&self, id: usize) -> &[String] {
        self.panes.get(&id).map_or(&[], |p| p.tabs.as_slice())
    }

    pub fn current(&self, id: usize) -> Option<&str> {
        self.panes.get(&id)?.current.as_deref()
    }

    /// 木の中のペインを、左上から順に
    pub fn order(&self) -> Vec<usize> {
        fn walk(n: &Node, out: &mut Vec<usize>) {
            match n {
                Node::Pane(id) => out.push(*id),
                Node::Split { children, .. } => children.iter().for_each(|c| walk(c, out)),
            }
        }
        let mut out = Vec::new();
        walk(&self.root, &mut out);
        out
    }

    /// 開いているノートとアクティブなノートに合わせる（毎回の描画の前に呼ぶ）。
    /// 閉じたノートを除き、どのペインにも無いノートはフォーカスのあるペインに入れ、空のペインを閉じる
    pub fn sync(&mut self, open: &[String], active: Option<&str>) {
        self.repair();
        for p in self.panes.values_mut() {
            p.tabs.retain(|t| open.contains(t));
        }
        if let Some(id) = active.and_then(|a| self.pane_of(a)) {
            self.focused = id;
        }
        for path in open {
            if self.pane_of(path).is_none() {
                let pane = self.panes.entry(self.focused).or_default();
                // 表示中のタブの後ろに入れる
                let at = pane
                    .current
                    .as_ref()
                    .and_then(|c| pane.tabs.iter().position(|t| t == c))
                    .map_or(pane.tabs.len(), |i| i + 1);
                pane.tabs.insert(at, path.clone());
            }
        }
        if let Some(a) = active {
            if let Some(id) = self.pane_of(a) {
                self.focused = id;
                self.panes.entry(id).or_default().current = Some(a.to_string());
            }
        }
        let empty: Vec<usize> = self
            .panes
            .iter()
            .filter(|(_, p)| p.tabs.is_empty())
            .map(|(&id, _)| id)
            .collect();
        for id in empty {
            if self.panes.len() > 1 {
                self.remove_pane(id);
            }
        }
        for p in self.panes.values_mut() {
            if !p.current.as_ref().is_some_and(|c| p.tabs.contains(c)) {
                p.current = p.tabs.first().cloned();
            }
        }
    }

    /// 木とペインの表が食い違っていたら直す（読み込んだ設定が壊れていたときなど）
    fn repair(&mut self) {
        let order = self.order();
        let valid = !order.is_empty()
            && order.iter().all(|id| self.panes.contains_key(id))
            && order.len() == self.panes.len();
        if !valid {
            let tabs: Vec<String> = self
                .panes
                .values()
                .flat_map(|p| p.tabs.iter().cloned())
                .collect();
            *self = Self::default();
            self.panes.entry(0).or_default().tabs = tabs;
        }
        if !self.panes.contains_key(&self.focused) {
            self.focused = self.order()[0];
        }
    }

    fn new_id(&self) -> usize {
        self.panes.keys().max().map_or(0, |m| m + 1)
    }

    /// フォーカスのあるペインを `dir` の向きに分け、新しいペインに `path` を出す
    /// （ほかのペインにあれば移す）。新しいペインにフォーカスを移し、その番号を返す
    pub fn split(&mut self, dir: Dir, path: &str) -> usize {
        self.remove_tab(path);
        let id = self.new_id();
        let target = self.focused;
        if !insert_next(&mut self.root, target, id, dir) {
            // 見つからなければ、全体を分ける
            let old = std::mem::replace(&mut self.root, Node::Pane(0));
            self.root = Node::Split {
                dir,
                children: vec![old, Node::Pane(id)],
                sizes: vec![0.5, 0.5],
            };
        }
        self.panes.insert(
            id,
            Pane {
                tabs: vec![path.to_string()],
                current: Some(path.to_string()),
            },
        );
        self.focused = id;
        id
    }

    /// ペインを閉じる。タブは隣のペインに移し、そこにフォーカスを移す
    pub fn close(&mut self, id: usize) {
        if self.panes.len() <= 1 {
            return;
        }
        let order = self.order();
        let i = order.iter().position(|&p| p == id).unwrap_or(0);
        let to = if i > 0 { order[i - 1] } else { order[1] };
        let tabs = self
            .panes
            .get(&id)
            .map(|p| p.tabs.clone())
            .unwrap_or_default();
        self.panes.entry(to).or_default().tabs.extend(tabs);
        self.remove_pane(id);
        self.focused = to;
    }

    fn remove_pane(&mut self, id: usize) {
        self.panes.remove(&id);
        remove_leaf(&mut self.root, id);
        if !self.panes.contains_key(&self.focused) {
            self.focused = self.order().first().copied().unwrap_or(0);
        }
    }

    fn remove_tab(&mut self, path: &str) {
        for p in self.panes.values_mut() {
            p.tabs.retain(|t| t != path);
            if p.current.as_deref() == Some(path) {
                p.current = None;
            }
        }
    }

    /// タブを `to` のペインの、`before` の前（無ければ最後）に移す。移した先にフォーカスを移す
    pub fn move_tab(&mut self, path: &str, to: usize, before: Option<&str>) {
        if !self.panes.contains_key(&to) || before == Some(path) {
            return;
        }
        let from = self.pane_of(path);
        self.remove_tab(path);
        let pane = self.panes.entry(to).or_default();
        let at = before
            .and_then(|b| pane.tabs.iter().position(|t| t == b))
            .unwrap_or(pane.tabs.len());
        pane.tabs.insert(at, path.to_string());
        pane.current = Some(path.to_string());
        self.focused = to;
        // 空になったペインを閉じる
        if let Some(from) = from.filter(|&f| f != to) {
            if self.tabs(from).is_empty() {
                self.remove_pane(from);
            }
        }
    }

    /// 同じペインの、次（前）のタブ
    pub fn cycle(&self, path: &str, forward: bool) -> Option<String> {
        let tabs = self.tabs(self.pane_of(path)?);
        let i = tabs.iter().position(|t| t == path)?;
        let n = tabs.len();
        let j = if forward {
            (i + 1) % n
        } else {
            (i + n - 1) % n
        };
        tabs.get(j).cloned()
    }

    /// 次のペイン（左上から順に、最後の次は最初）
    pub fn next_pane(&self) -> Option<usize> {
        let order = self.order();
        let i = order.iter().position(|&p| p == self.focused)?;
        (order.len() > 1).then(|| order[(i + 1) % order.len()])
    }

    /// ノートの改名・移動に合わせる
    pub fn rename(&mut self, rename: impl Fn(&str) -> Option<String>) {
        for p in self.panes.values_mut() {
            for t in p.tabs.iter_mut().chain(p.current.iter_mut()) {
                if let Some(new) = rename(t) {
                    *t = new;
                }
            }
        }
    }

    /// `path` の枝の、`index` 番目と次の子の境目を `delta`（割合）だけ動かす
    pub fn resize(&mut self, path: &[usize], index: usize, delta: f32) {
        let mut node = &mut self.root;
        for &i in path {
            match node {
                Node::Split { children, .. } if i < children.len() => node = &mut children[i],
                _ => return,
            }
        }
        if let Node::Split { sizes, .. } = node {
            if index + 1 < sizes.len() {
                let total = sizes[index] + sizes[index + 1];
                let a = (sizes[index] + delta).clamp(MIN_SIZE, total - MIN_SIZE);
                sizes[index] = a;
                sizes[index + 1] = total - a;
            }
        }
    }

    /// 前の版の設定（タブの並びと、もう一方のペインのノート）から作る
    pub fn from_legacy(tabs: &[String], other: Option<&str>, focus_right: bool) -> Self {
        let mut panes = Self::default();
        panes.panes.entry(0).or_default().tabs = tabs
            .iter()
            .filter(|t| Some(t.as_str()) != other)
            .cloned()
            .collect();
        if let Some(other) = other.filter(|o| tabs.iter().any(|t| t == o)) {
            panes.split(Dir::Row, other);
            if !focus_right {
                panes.focused = 0;
            }
        }
        panes
    }
}

/// `target` のペインの隣（`dir` の向き）に `new` のペインを入れる
fn insert_next(node: &mut Node, target: usize, new: usize, dir: Dir) -> bool {
    match node {
        Node::Pane(id) if *id == target => {
            *node = Node::Split {
                dir,
                children: vec![Node::Pane(target), Node::Pane(new)],
                sizes: vec![0.5, 0.5],
            };
            true
        }
        Node::Pane(_) => false,
        Node::Split {
            dir: d,
            children,
            sizes,
        } => {
            // 同じ向きの分割の直下なら、その並びに入れる（入れ子にしない）
            if *d == dir {
                if let Some(i) = children
                    .iter()
                    .position(|c| matches!(c, Node::Pane(id) if *id == target))
                {
                    let half = sizes[i] / 2.0;
                    sizes[i] = half;
                    sizes.insert(i + 1, half);
                    children.insert(i + 1, Node::Pane(new));
                    return true;
                }
            }
            children
                .iter_mut()
                .any(|c| insert_next(c, target, new, dir))
        }
    }
}

/// ペインを木から除き、子が1つになった分割を畳む
fn remove_leaf(node: &mut Node, id: usize) {
    if let Node::Split {
        dir,
        children,
        sizes,
    } = node
    {
        if let Some(i) = children
            .iter()
            .position(|c| matches!(c, Node::Pane(p) if *p == id))
        {
            children.remove(i);
            let freed = sizes.remove(i);
            // 空いた分を残りに配る
            let n = sizes.len() as f32;
            sizes.iter_mut().for_each(|s| *s += freed / n);
        } else {
            children.iter_mut().for_each(|c| remove_leaf(c, id));
        }
        // 同じ向きの入れ子を平らにする
        let mut flat_children = Vec::new();
        let mut flat_sizes = Vec::new();
        for (c, s) in children.drain(..).zip(sizes.drain(..)) {
            match c {
                Node::Split {
                    dir: d,
                    children: cc,
                    sizes: cs,
                } if d == *dir => {
                    flat_sizes.extend(cs.iter().map(|x| x * s));
                    flat_children.extend(cc);
                }
                other => {
                    flat_children.push(other);
                    flat_sizes.push(s);
                }
            }
        }
        *children = flat_children;
        *sizes = flat_sizes;
        if children.len() == 1 {
            *node = children.remove(0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open(paths: &[&str]) -> Vec<String> {
        paths.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn split_and_close() {
        let mut p = Panes::default();
        p.sync(&open(&["a", "b", "c"]), Some("a"));
        assert_eq!(p.tabs(0), ["a", "b", "c"]);
        assert!(!p.is_split());

        // 右に分けて b を出す
        let right = p.split(Dir::Row, "b");
        assert_eq!(p.focused, right);
        assert_eq!(p.tabs(0), ["a", "c"]);
        assert_eq!(p.tabs(right), ["b"]);
        // その下に分けて c を出す
        let below = p.split(Dir::Column, "c");
        assert_eq!(p.order(), [0, right, below]);
        assert!(
            matches!(&p.root, Node::Split { dir: Dir::Row, children, .. } if matches!(children[1], Node::Split { dir: Dir::Column, .. }))
        );
        // さらに右に分けると、同じ向きの並びに入る（入れ子にしない）
        p.sync(&open(&["a", "b", "c", "d"]), Some("a"));
        let third = p.split(Dir::Row, "d");
        assert!(matches!(&p.root, Node::Split { children, .. } if children.len() == 3));
        assert_eq!(p.order(), [0, third, right, below]);

        // 閉じると、タブは隣のペインに移り、分割が畳まれる
        p.close(below);
        assert_eq!(p.tabs(right), ["b", "c"]);
        assert_eq!(p.order(), [0, third, right]);
        // ノートを閉じてペインが空になったら、ペインも閉じる
        p.sync(&open(&["a", "b", "c"]), Some("a"));
        assert_eq!(p.order(), [0, right]);
        p.sync(&open(&["a"]), Some("a"));
        assert!(!p.is_split());
        assert_eq!(p.root, Node::Pane(0));
    }

    #[test]
    fn new_tabs_go_to_the_focused_pane() {
        let mut p = Panes::default();
        p.sync(&open(&["a", "b"]), Some("a"));
        let right = p.split(Dir::Row, "b");
        p.sync(&open(&["a", "b", "x"]), Some("b"));
        assert_eq!(p.tabs(right), ["b", "x"]);
        // アクティブなタブのあるペインにフォーカスが移る
        p.sync(&open(&["a", "b", "x"]), Some("a"));
        assert_eq!(p.focused, 0);
        assert_eq!(p.current(right), Some("b"));
        assert_eq!(p.cycle("b", true).as_deref(), Some("x"));
        assert_eq!(p.cycle("b", false).as_deref(), Some("x"));
        assert_eq!(p.next_pane(), Some(right));
    }

    #[test]
    fn move_tabs_between_panes() {
        let mut p = Panes::default();
        p.sync(&open(&["a", "b", "c"]), Some("a"));
        let right = p.split(Dir::Row, "c");
        // 並べ替え
        p.move_tab("b", 0, Some("a"));
        assert_eq!(p.tabs(0), ["b", "a"]);
        // 別のペインへ
        p.move_tab("a", right, None);
        assert_eq!(p.tabs(right), ["c", "a"]);
        assert_eq!(p.focused, right);
        // 最後のタブを移すと、元のペインは閉じる
        p.move_tab("b", right, Some("c"));
        assert_eq!(p.tabs(right), ["b", "c", "a"]);
        assert!(!p.is_split());
    }

    #[test]
    fn resize_and_legacy() {
        let mut p = Panes::from_legacy(&open(&["a", "b"]), Some("b"), false);
        assert_eq!(p.focused, 0);
        assert_eq!(p.tabs(0), ["a"]);
        p.resize(&[], 0, 0.2);
        assert!(matches!(&p.root, Node::Split { sizes, .. } if (sizes[0] - 0.7).abs() < 1e-6));
        p.resize(&[], 0, 1.0);
        assert!(matches!(&p.root, Node::Split { sizes, .. } if (sizes[1] - MIN_SIZE).abs() < 1e-6));
        // 保存して読み直せる
        let json = serde_json::to_string(&p).unwrap();
        assert_eq!(serde_json::from_str::<Panes>(&json).unwrap(), p);
        // 壊れた設定は直す
        let mut broken = p.clone();
        broken.panes.remove(&0);
        broken.sync(&open(&["a", "b"]), Some("a"));
        assert!(!broken.is_split());
        assert_eq!(broken.tabs(0).len(), 2);
    }
}
