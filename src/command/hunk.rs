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
        HunkCommand::Fold {
            target,
            from,
            allow_pushed,
        } => hunk::fold(&target, &from, allow_pushed),
        HunkCommand::Squash {
            commit,
            message,
            force,
            no_preserve_author,
            allow_pushed,
        } => hunk::squash(&commit, &message, force, !no_preserve_author, allow_pushed),
        HunkCommand::Move {
            commit,
            after,
            before,
            to_end,
            allow_pushed,
        } => {
            let to = match (&after, &before, to_end) {
                (Some(r), _, _) => hunk::MoveTo::After(r),
                (_, Some(r), _) => hunk::MoveTo::Before(r),
                (_, _, true) => hunk::MoveTo::End,
                _ => anyhow::bail!("one of --after, --before or --to-end is required"),
            };
            hunk::move_commit(&commit, to, allow_pushed)
        }
        HunkCommand::Split { commit, args } => {
            hunk::split(&commit, &hunk::parse_split_args(&args)?)
        }
        HunkCommand::Commit { message, ids } => hunk::commit(&ids, &message),
        HunkCommand::CommitTo {
            branch,
            message,
            ids,
        } => hunk::commit_to(&branch, &ids, &message),
    }
}
