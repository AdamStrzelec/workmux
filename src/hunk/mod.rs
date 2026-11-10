//! Hunk-level git operations for agents: list, show, stage, unstage, discard,
//! commit and commit-to (another branch, without checking it out), plus undo,
//! amend, reword, fold, squash, move and split for earlier commits.
//!
//! Hunk IDs are short content hashes (file path + hunk body), so they survive
//! line shifts. Only tracked-file changes appear in `git diff`; new files must
//! be added with `git add -N <file>` first.

mod blame;
mod diff;
mod history;
mod id;
mod ops;
mod patch;
mod rewrite;

pub use history::{amend, reword};
pub use ops::{commit, commit_to, discard, list, parse_line_range, show, stage, undo, unstage};
pub use rewrite::{MoveTo, fold, move_commit, parse_split_args, split, squash};
