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
        } => hunk::list(staged, file.as_deref(), commit.as_deref(), full),
        HunkCommand::Show { id, commit } => hunk::show(&id, commit.as_deref()),
        HunkCommand::Stage { ids, lines } => hunk::stage(&ids, lines),
        HunkCommand::Unstage { ids, lines } => hunk::unstage(&ids, lines),
        HunkCommand::Discard { ids, lines } => hunk::discard(&ids, lines),
        HunkCommand::Commit { message, ids } => hunk::commit(&ids, &message),
        HunkCommand::CommitTo {
            branch,
            message,
            ids,
        } => hunk::commit_to(&branch, &ids, &message),
    }
}
