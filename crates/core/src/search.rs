//! ノート名のあいまい検索と、本文の全文検索。
//!
//! 本文はすべてメモリ上にあるので、全文検索はディスクI/Oなしで全コア並列に走る。

use crate::task::Task;
use crate::vault::Vault;
use chrono::NaiveDate;
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use rayon::prelude::*;
use regex::{Regex, RegexBuilder};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct LineHit {
    /// 0始まりの行番号
    pub line: usize,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct NoteHit {
    pub path: String,
    pub score: u32,
    pub lines: Vec<LineHit>,
}

/// ノートのパスをあいまい検索する（クイックスイッチャー用）。
/// クエリが空のときは更新が新しい順に返す。
pub fn fuzzy(vault: &Vault, query: &str, limit: usize) -> Vec<String> {
    let query = query.trim();
    if query.is_empty() {
        let mut notes: Vec<_> = vault.notes().collect();
        notes.sort_by(|a, b| b.mtime.cmp(&a.mtime).then_with(|| a.path.cmp(&b.path)));
        return notes
            .into_iter()
            .take(limit)
            .map(|n| n.path.clone())
            .collect();
    }
    let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
    let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
    let mut buf = Vec::new();
    let mut scored: Vec<(u32, &str)> = Vec::new();
    for note in vault.notes() {
        let hay = note.path.strip_suffix(".md").unwrap_or(&note.path);
        let path_score = pattern.score(Utf32Str::new(hay, &mut buf), &mut matcher);
        // ファイル名そのものに当たる候補を、フォルダ名に当たる候補より上に出す
        let name_score = pattern
            .score(Utf32Str::new(&note.name, &mut buf), &mut matcher)
            .map(|s| s * 2);
        let alias_score = note
            .parsed
            .aliases
            .iter()
            .filter_map(|a| pattern.score(Utf32Str::new(a, &mut buf), &mut matcher))
            .max();
        if let Some(score) = [path_score, name_score, alias_score]
            .into_iter()
            .flatten()
            .max()
        {
            scored.push((score, &note.path));
        }
    }
    scored.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| a.1.len().cmp(&b.1.len()))
            .then_with(|| a.1.cmp(b.1))
    });
    scored
        .into_iter()
        .take(limit)
        .map(|(_, p)| p.to_string())
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TaskFilter {
    Todo,
    Done,
    Any,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DueFilter {
    Overdue,
    Today,
    /// 今日から7日以内（期限切れを含む）
    Week,
    None,
    Some,
    Before(NaiveDate, bool),
    After(NaiveDate, bool),
    On(NaiveDate),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sort {
    Score,
    Name,
    Path,
    /// 更新が新しい順
    Mtime,
    Due,
}

#[derive(Debug, Default)]
struct Query {
    terms: Vec<Regex>,
    tags: Vec<String>,
    paths: Vec<String>,
    task: Option<TaskFilter>,
    due: Vec<DueFilter>,
    sort: Option<Sort>,
    limit: Option<usize>,
    /// 解釈できなかった条件（呼び出し側で知らせる）
    errors: Vec<String>,
}

impl Query {
    fn is_empty(&self) -> bool {
        self.terms.is_empty()
            && self.tags.is_empty()
            && self.paths.is_empty()
            && self.task.is_none()
            && self.due.is_empty()
            && self.sort.is_none()
    }

    fn tasks_mode(&self) -> bool {
        self.task.is_some() || !self.due.is_empty()
    }
}

fn parse_date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
}

fn parse_due(v: &str) -> Option<DueFilter> {
    Some(match v {
        "overdue" => DueFilter::Overdue,
        "today" => DueFilter::Today,
        "week" => DueFilter::Week,
        "none" => DueFilter::None,
        "any" | "some" => DueFilter::Some,
        _ => {
            if let Some(d) = v.strip_prefix("<=") {
                DueFilter::Before(parse_date(d)?, true)
            } else if let Some(d) = v.strip_prefix('<') {
                DueFilter::Before(parse_date(d)?, false)
            } else if let Some(d) = v.strip_prefix(">=") {
                DueFilter::After(parse_date(d)?, true)
            } else if let Some(d) = v.strip_prefix('>') {
                DueFilter::After(parse_date(d)?, false)
            } else {
                DueFilter::On(parse_date(v)?)
            }
        }
    })
}

/// 空白区切りのAND検索。`"フレーズ"`、`tag:名前`、`path:部分文字列` に加えて、
/// `task:todo|done|any`、`due:overdue|today|week|none|any|<日付|<=日付|>日付|日付`、
/// `sort:score|name|path|mtime|due`、`limit:件数` に対応。
fn parse_query(query: &str) -> Query {
    let mut q = Query::default();
    let mut tokens: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    for c in query.chars() {
        match c {
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if !cur.is_empty() {
                    tokens.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        tokens.push(cur);
    }
    for t in tokens {
        if let Some(tag) = t.strip_prefix("tag:") {
            let tag = tag.trim_start_matches('#').to_lowercase();
            if !tag.is_empty() {
                q.tags.push(tag);
            }
        } else if let Some(p) = t.strip_prefix("path:") {
            if !p.is_empty() {
                q.paths.push(p.to_lowercase());
            }
        } else if let Some(v) = t.strip_prefix("task:") {
            match v {
                "todo" | "open" | "" => q.task = Some(TaskFilter::Todo),
                "done" => q.task = Some(TaskFilter::Done),
                "any" | "all" => q.task = Some(TaskFilter::Any),
                _ => q.errors.push(t.clone()),
            }
        } else if let Some(v) = t.strip_prefix("due:") {
            match parse_due(v) {
                Some(d) => q.due.push(d),
                None => q.errors.push(t.clone()),
            }
        } else if let Some(v) = t.strip_prefix("sort:") {
            q.sort = Some(match v {
                "score" => Sort::Score,
                "name" => Sort::Name,
                "path" => Sort::Path,
                "mtime" | "modified" | "updated" => Sort::Mtime,
                "due" => Sort::Due,
                _ => {
                    q.errors.push(t.clone());
                    continue;
                }
            });
        } else if let Some(v) = t.strip_prefix("limit:") {
            match v.parse() {
                Ok(n) => q.limit = Some(n),
                Err(_) => q.errors.push(t.clone()),
            }
        } else if let Ok(re) = RegexBuilder::new(&regex::escape(&t))
            .case_insensitive(true)
            .build()
        {
            q.terms.push(re);
        }
    }
    q
}

/// 見つかったタスク。
#[derive(Debug, Clone, Serialize)]
pub struct TaskHit {
    pub path: String,
    pub task: Task,
}

/// [`run_query`] の結果。タスクの条件（`task:` / `due:`）があればタスク、無ければノート。
#[derive(Debug, Clone)]
pub enum QueryResult {
    Notes(Vec<NoteHit>),
    Tasks(Vec<TaskHit>),
}

/// 条件の書き間違い（`due:tomorow` など）。
pub fn query_errors(query: &str) -> Vec<String> {
    parse_query(query).errors
}

fn due_matches(f: DueFilter, task: &Task, today: NaiveDate) -> bool {
    let due = task.due;
    match f {
        DueFilter::Overdue => due.is_some_and(|d| d < today) && !task.done(),
        DueFilter::Today => due == Some(today),
        DueFilter::Week => due.is_some_and(|d| d <= today + chrono::Days::new(7)),
        DueFilter::None => due.is_none(),
        DueFilter::Some => due.is_some(),
        DueFilter::Before(x, inclusive) => due.is_some_and(|d| d < x || (inclusive && d == x)),
        DueFilter::After(x, inclusive) => due.is_some_and(|d| d > x || (inclusive && d == x)),
        DueFilter::On(x) => due == Some(x),
    }
}

/// 検索条件を実行する。`today` は期日の判定に使う。
pub fn run_query(vault: &Vault, query: &str, today: NaiveDate) -> QueryResult {
    const MAX_LINES_PER_NOTE: usize = 20;
    let q = parse_query(query);
    if q.is_empty() {
        return QueryResult::Notes(Vec::new());
    }
    let notes: Vec<_> = vault
        .notes()
        .filter(|note| {
            let lower_path = note.path.to_lowercase();
            if !q.paths.iter().all(|p| lower_path.contains(p)) {
                return false;
            }
            // `tag:a` は子タグ `a/b` にも一致させる
            q.tags.iter().all(|want| {
                note.parsed.tags.iter().any(|t| {
                    let t = t.to_lowercase();
                    t == *want
                        || t.strip_prefix(want.as_str())
                            .is_some_and(|r| r.starts_with('/'))
                })
            })
        })
        .collect();

    if q.tasks_mode() {
        let filter = q.task.unwrap_or(TaskFilter::Todo);
        let mut hits: Vec<(u64, TaskHit)> = notes
            .iter()
            .flat_map(|note| {
                note.parsed.tasks.iter().filter_map(|task| {
                    let status_ok = match filter {
                        TaskFilter::Todo => !task.done(),
                        TaskFilter::Done => task.done(),
                        TaskFilter::Any => true,
                    };
                    let ok = status_ok
                        && q.due.iter().all(|f| due_matches(*f, task, today))
                        && q.terms.iter().all(|re| re.is_match(&task.text));
                    ok.then(|| {
                        let hit = TaskHit {
                            path: note.path.clone(),
                            task: task.clone(),
                        };
                        (note.mtime, hit)
                    })
                })
            })
            .collect();
        match q.sort.unwrap_or(Sort::Due) {
            Sort::Mtime => {
                hits.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.task.line.cmp(&b.1.task.line)))
            }
            Sort::Name | Sort::Path | Sort::Score => hits.sort_by(|a, b| {
                (a.1.path.as_str(), a.1.task.line).cmp(&(b.1.path.as_str(), b.1.task.line))
            }),
            Sort::Due => hits.sort_by(|a, b| {
                // 期日の無いものは最後
                let key = |h: &TaskHit| {
                    (
                        h.task.due.is_none(),
                        h.task.due,
                        h.path.clone(),
                        h.task.line,
                    )
                };
                key(&a.1).cmp(&key(&b.1))
            }),
        }
        let mut hits: Vec<TaskHit> = hits.into_iter().map(|(_, h)| h).collect();
        if let Some(n) = q.limit {
            hits.truncate(n);
        }
        return QueryResult::Tasks(hits);
    }

    let mut hits: Vec<(u64, String, NoteHit)> = notes
        .par_iter()
        .filter_map(|note| {
            let mut score = 1u32;
            for re in &q.terms {
                let in_name = re.is_match(&note.name);
                let count = re.find_iter(&note.text).take(50).count() as u32;
                if !in_name && count == 0 {
                    return None;
                }
                score += count + if in_name { 100 } else { 0 };
            }
            let mut lines = Vec::new();
            if !q.terms.is_empty() {
                for (i, line) in note.text.lines().enumerate() {
                    if q.terms.iter().any(|re| re.is_match(line)) {
                        lines.push(LineHit {
                            line: i,
                            text: line.trim().to_string(),
                        });
                        if lines.len() >= MAX_LINES_PER_NOTE {
                            break;
                        }
                    }
                }
            }
            let hit = NoteHit {
                path: note.path.clone(),
                score,
                lines,
            };
            Some((note.mtime, note.name.to_lowercase(), hit))
        })
        .collect();
    match q.sort.unwrap_or(Sort::Score) {
        Sort::Score | Sort::Due => hits.sort_by(|a, b| {
            b.2.score
                .cmp(&a.2.score)
                .then_with(|| a.2.path.cmp(&b.2.path))
        }),
        Sort::Mtime => hits.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.2.path.cmp(&b.2.path))),
        Sort::Name => hits.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.2.path.cmp(&b.2.path))),
        Sort::Path => hits.sort_by(|a, b| a.2.path.cmp(&b.2.path)),
    }
    let mut hits: Vec<NoteHit> = hits.into_iter().map(|(_, _, h)| h).collect();
    if let Some(n) = q.limit {
        hits.truncate(n);
    }
    QueryResult::Notes(hits)
}

/// 全文検索。すべての条件を満たすノートを返す（既定は関連度の高い順）。
/// タスクの条件があれば、該当するタスクを含むノートを、タスクの行を一致行として返す。
pub fn search(vault: &Vault, query: &str, limit: usize) -> Vec<NoteHit> {
    let today = chrono::Local::now().date_naive();
    let mut hits = match run_query(vault, query, today) {
        QueryResult::Notes(hits) => hits,
        QueryResult::Tasks(tasks) => {
            let mut out: Vec<NoteHit> = Vec::new();
            for t in tasks {
                let line = LineHit {
                    line: t.task.line,
                    text: vault.line(&t.path, t.task.line).to_string(),
                };
                match out.iter_mut().find(|h| h.path == t.path) {
                    Some(h) => h.lines.push(line),
                    None => out.push(NoteHit {
                        path: t.path,
                        score: 1,
                        lines: vec![line],
                    }),
                }
            }
            out
        }
    };
    hits.truncate(limit);
    hits
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn vault() -> (tempfile::TempDir, Vault) {
        let dir = tempfile::tempdir().unwrap();
        let files = [
            ("Rust メモ.md", "所有権について\nBorrow checker #lang/rust"),
            ("projects/robsidian.md", "Rust製のノートアプリ\nrust RUST"),
            ("daily/2026-01-01.md", "今日は休み #daily"),
        ];
        for (p, t) in files {
            let abs = dir.path().join(p);
            fs::create_dir_all(abs.parent().unwrap()).unwrap();
            fs::write(abs, t).unwrap();
        }
        let v = Vault::open(dir.path()).unwrap();
        (dir, v)
    }

    #[test]
    fn fuzzy_names() {
        let (_d, v) = vault();
        assert_eq!(fuzzy(&v, "robs", 10), ["projects/robsidian.md"]);
        assert_eq!(fuzzy(&v, "メモ", 10), ["Rust メモ.md"]);
        assert_eq!(fuzzy(&v, "", 10).len(), 3);
        assert!(fuzzy(&v, "zzzz", 10).is_empty());
    }

    #[test]
    fn task_queries() {
        let dir = tempfile::tempdir().unwrap();
        let files = [
            (
                "work/a.md",
                "#project\n- [ ] 予算を出す 📅 2026-01-10\n- [x] 済み 📅 2026-01-01\n- [ ] いつか",
            ),
            (
                "b.md",
                "- [ ] 電話 due:2026-01-05\n- [ ] 予算の相談 due:2026-01-20",
            ),
        ];
        for (p, t) in files {
            let abs = dir.path().join(p);
            fs::create_dir_all(abs.parent().unwrap()).unwrap();
            fs::write(abs, t).unwrap();
        }
        let v = Vault::open(dir.path()).unwrap();
        let today = NaiveDate::from_ymd_opt(2026, 1, 8).unwrap();
        let tasks = |q: &str| match run_query(&v, q, today) {
            QueryResult::Tasks(t) => t.into_iter().map(|h| h.task.text).collect::<Vec<_>>(),
            QueryResult::Notes(_) => panic!("expected tasks for {q}"),
        };
        // 既定は未完了だけを、期日の近い順（期日なしは最後）
        assert_eq!(
            tasks("task:todo"),
            ["電話", "予算を出す", "予算の相談", "いつか"]
        );
        assert_eq!(tasks("task:done"), ["済み"]);
        assert_eq!(tasks("due:overdue"), ["電話"]);
        assert_eq!(tasks("due:week"), ["電話", "予算を出す"]);
        assert_eq!(tasks("due:none"), ["いつか"]);
        assert_eq!(tasks("due:<2026-01-11"), ["電話", "予算を出す"]);
        assert_eq!(tasks("task:any due:2026-01-01"), ["済み"]);
        assert_eq!(tasks("task:todo 予算"), ["予算を出す", "予算の相談"]);
        assert_eq!(tasks("task:todo tag:project limit:1"), ["予算を出す"]);
        assert_eq!(
            query_errors("due:tomorow sort:x task:todo"),
            ["due:tomorow", "sort:x"]
        );

        let notes = |q: &str| match run_query(&v, q, today) {
            QueryResult::Notes(n) => n.into_iter().map(|h| h.path).collect::<Vec<_>>(),
            QueryResult::Tasks(_) => panic!("expected notes"),
        };
        assert_eq!(notes("sort:name"), ["work/a.md", "b.md"]);
        assert_eq!(notes("sort:path limit:1"), ["b.md"]);
        // 検索欄でタスクの条件を使うと、タスクの行が一致行になる
        let hits = search(&v, "task:todo 予算", 10);
        assert_eq!(hits.len(), 2);
        assert!(hits[0].lines[0].text.contains("予算"));
    }

    #[test]
    fn full_text() {
        let (_d, v) = vault();
        let hits = search(&v, "rust", 10);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].path, "Rust メモ.md"); // ファイル名一致が上
        assert_eq!(hits[1].lines.len(), 2);
        assert_eq!(search(&v, "rust ノート", 10).len(), 1);
        assert_eq!(search(&v, "\"borrow checker\"", 10).len(), 1);
        assert_eq!(search(&v, "tag:lang", 10)[0].path, "Rust メモ.md");
        assert_eq!(search(&v, "path:daily", 10).len(), 1);
        assert!(search(&v, "  ", 10).is_empty());
    }
}
