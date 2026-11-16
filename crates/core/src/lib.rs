//! Robsidianのコア: vaultの索引・リンクグラフ・検索。
//!
//! GUI（`robsidian`）とCLI（`rob`）の両方がこのクレートだけに依存する。

pub mod parse;
pub mod search;
pub mod vault;

pub use parse::{Heading, Link, LinkKind, Parsed};
pub use search::{LineHit, NoteHit};
pub use vault::{Backlink, Note, Vault};
