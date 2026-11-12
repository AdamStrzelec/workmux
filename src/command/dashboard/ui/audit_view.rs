//! Read-only overlay with the newest audit log events.

use ratatui::{
    Frame,
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
};

use super::popup::{centered_rect, popup_block, render_popup};
use crate::command::dashboard::app::App;

pub fn render_audit_view(f: &mut Frame, app: &App) {
    let Some(lines) = &app.audit_view else { return };
    let p = &app.palette;
    let area = f.area();
    let width = area.width.saturating_sub(4).max(20);
    let height = area.height.saturating_sub(4).max(5);
    let rect = centered_rect(area, width, height);
    let visible = height.saturating_sub(4) as usize;
    let mut out: Vec<Line> = lines
        .iter()
        .take(visible)
        .map(|l| {
            let style = if l.ends_with("FAILED") {
                Style::default().fg(p.danger)
            } else {
                Style::default()
            };
            Line::from(Span::styled(l.clone(), style))
        })
        .collect();
    out.push(Line::from(""));
    out.push(Line::from(Span::styled(
        "Esc/Enter: close",
        Style::default().fg(p.dimmed),
    )));
    render_popup(
        f,
        rect,
        Paragraph::new(out).block(popup_block(Some("Audit log"), p)),
    );
}
