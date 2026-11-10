//! Hunk-level git operations for agents: list, show, stage, unstage, discard,
//! commit and commit-to (another branch, without checking it out).
//!
//! Hunk IDs are short content hashes (file path + hunk body), so they survive
//! line shifts. Only tracked-file changes appear in `git diff`; new files must
//! be added with `git add -N <file>` first.

mod blame;
mod diff;
mod id;
mod ops;
mod patch;

pub use ops::{commit, commit_to, discard, list, parse_line_range, show, stage, unstage};
