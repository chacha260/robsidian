//! Robsidianのコア: vaultの索引・リンクグラフ・検索。
//!
//! GUI（`robsidian`）とCLI（`rob`）の両方がこのクレートだけに依存する。

pub mod bases;
pub mod canvas;
pub mod formula;
pub mod git;
pub mod history;
pub mod math;
pub mod parse;
pub mod props;
pub mod related;
pub mod search;
pub mod task;
pub mod template;
pub mod text;
pub mod theme;
pub mod usercmd;
pub mod vault;

pub use parse::{Heading, Link, LinkKind, Parsed};
pub use search::{LineHit, NoteHit};
pub use task::Task;
pub use vault::{Backlink, Mention, Note, Vault};
