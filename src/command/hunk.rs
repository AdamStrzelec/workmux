use anyhow::Result;

use crate::cli::HunkCommand;
use crate::hunk;

pub fn run(command: HunkCommand) -> Result<()> {
    match command {
        HunkCommand::List {
            staged,
            file,
            commit,
            full,
            blame,
        } => hunk::list(staged, file.as_deref(), commit.as_deref(), full, blame),
        HunkCommand::Show { id, commit } => hunk::show(&id, commit.as_deref()),
        HunkCommand::Stage { ids, lines } => hunk::stage(&ids, lines),
        HunkCommand::Unstage { ids, lines } => hunk::unstage(&ids, lines),
        HunkCommand::Discard { ids, lines } => hunk::discard(&ids, lines),
        HunkCommand::Undo { ids, from, lines } => hunk::undo(&ids, &from, lines),
        HunkCommand::Amend {
            commit,
            allow_pushed,
        } => hunk::amend(&commit, allow_pushed),
        HunkCommand::Reword {
            commit,
            message,
            allow_pushed,
        } => hunk::reword(&commit, &message, allow_pushed),
        HunkCommand::Commit { message, ids } => hunk::commit(&ids, &message),
        HunkCommand::CommitTo {
            branch,
            message,
            ids,
        } => hunk::commit_to(&branch, &ids, &message),
    }
}
