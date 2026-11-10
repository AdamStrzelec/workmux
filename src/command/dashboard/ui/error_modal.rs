//! Error modal: errors stay visible until the user confirms with OK.

use ratatui::{
    Frame,
    layout::Alignment,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Paragraph, Wrap},
};

use super::popup::{centered_rect, render_popup};
use crate::command::dashboard::app::App;

/// Heuristic: does a status message report a failure?
pub fn is_error_message(msg: &str) -> bool {
    let lower = msg.to_lowercase();
    [
        "fail",
        "error",
        "cannot",
        "can't",
        "could not",
        "couldn't",
        "unable",
        "aborted",
        "not found",
        "invalid",
        "denied",
        "requires",
        "✗",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

/// Move an error-looking status message into the modal so it does not expire.
pub fn promote_status_error(app: &mut App) {
    let is_error = matches!(&app.status_message, Some((msg, _)) if is_error_message(msg));
    if is_error && let Some((msg, _)) = app.status_message.take() {
        app.error_modal = Some(match app.error_modal.take() {
            Some(prev) if prev != msg => format!("{prev}\n\n{msg}"),
            _ => msg,
        });
    }
}

pub fn render_error_modal(f: &mut Frame, app: &App) {
    let Some(msg) = &app.error_modal else { return };
    let p = &app.palette;
    let red = Style::default().fg(p.danger);

    let area = f.area();
    let width = area.width.saturating_sub(4).clamp(20, 80);
    let inner_width = width.saturating_sub(4).max(1) as usize;
    let text_lines: usize = msg
        .lines()
        .map(|l| l.chars().count().div_ceil(inner_width).max(1))
        .sum();
    let height = (text_lines as u16 + 5).min(area.height);
    let rect = centered_rect(area, width, height);

    let mut lines: Vec<Line> = msg
        .lines()
        .map(|l| Line::from(Span::styled(l.to_string(), red)))
        .collect();
    lines.push(Line::from(""));
    lines.push(
        Line::from(Span::styled(
            "[ OK ]",
            red.add_modifier(Modifier::BOLD | Modifier::REVERSED),
        ))
        .alignment(Alignment::Center),
    );

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(red)
        .title(Span::styled(" Error ", red.add_modifier(Modifier::BOLD)));
    render_popup(
        f,
        rect,
        Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false }),
    );
}

#[cfg(test)]
mod tests {
    use super::is_error_message;

    #[test]
    fn detects_errors_but_not_info() {
        assert!(is_error_message("Failed to open 'x': boom"));
        assert!(is_error_message("Sweep failed: io"));
        assert!(is_error_message(
            "'windows' configuration requires 'mode: session'"
        ));
        assert!(is_error_message(
            "Move aborted: some worktrees cannot be moved"
        ));
        assert!(!is_error_message("Created worktree 'x'"));
        assert!(!is_error_message("Creating worktree 'x'..."));
        assert!(!is_error_message("Base for 'a' set to 'b'"));
    }
}
