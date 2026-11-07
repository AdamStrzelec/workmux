//! Run tools menu (worktrees tab), styled like the PR actions menu.

use ratatui::{
    Frame,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use super::super::app::App;
use super::popup::{centered_rect, popup_block, render_popup};

pub fn render_tool_menu(f: &mut Frame, app: &App) {
    let Some(menu) = &app.pending_tool_menu else {
        return;
    };
    let p = &app.palette;
    let bold = Style::default().fg(p.text).add_modifier(Modifier::BOLD);
    let muted = Style::default().fg(p.help_muted);
    let mut lines = vec![
        Line::from(Span::styled(
            format!(" {}", menu.handle),
            Style::default().fg(p.text),
        )),
        Line::from(""),
    ];
    for (tool, running) in menu.tools.iter().zip(&menu.running) {
        let valid = tool.key_char().is_some();
        let mut spans = vec![
            Span::styled(
                format!(" {:>5}", tool.key),
                if valid { bold } else { muted },
            ),
            Span::styled(" → ", muted),
            Span::styled(tool.name.clone(), Style::default().fg(p.text)),
            Span::styled(format!("  {}", tool.command), muted),
        ];
        if *running {
            spans.push(Span::styled(
                "  (running, jump)",
                Style::default().fg(p.success),
            ));
        } else if !valid {
            spans.push(Span::styled("  (key must be one character)", muted));
        }
        lines.push(Line::from(spans));
    }
    lines.push(Line::from(vec![
        Span::styled("   Esc", bold),
        Span::styled(" → ", muted),
        Span::styled("back", Style::default().fg(p.text)),
    ]));
    let area = f.area();
    let w = lines.iter().map(|l| l.width()).max().unwrap_or(0) as u16 + 4;
    let w = w.clamp(40, area.width.saturating_sub(4).max(40));
    let rect = centered_rect(area, w, lines.len() as u16 + 2);
    render_popup(
        f,
        rect,
        Paragraph::new(lines).block(popup_block(Some("Run tools"), p)),
    );
}
