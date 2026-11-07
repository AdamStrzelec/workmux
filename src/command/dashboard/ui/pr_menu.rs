//! PR action submenu and its confirmation popup (worktrees tab).

use ratatui::{
    Frame,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
};

use super::super::app::{App, display_cmd};
use super::popup::{centered_rect, popup_block, render_popup};

fn popup_width(lines: &[Line], area_w: u16) -> u16 {
    let w = lines.iter().map(|l| l.width()).max().unwrap_or(0) as u16 + 4;
    w.clamp(40, area_w.saturating_sub(4).max(40))
}

pub fn render_pr_menu(f: &mut Frame, app: &App) {
    let Some(menu) = &app.pending_pr_menu else {
        return;
    };
    let p = &app.palette;
    let t = &menu.target;
    let mut lines = vec![Line::from(Span::styled(
        match &t.pr {
            Some(pr) => format!(
                " {} - PR #{} ({}{})",
                t.branch,
                pr.number,
                pr.state.to_lowercase(),
                if pr.is_draft { ", draft" } else { "" }
            ),
            None => format!(" {} - no PR", t.branch),
        },
        Style::default().fg(p.text),
    ))];
    lines.push(Line::from(""));
    for (kind, reason) in menu.entries() {
        let enabled = reason.is_none();
        let key_style = if enabled {
            Style::default().fg(p.text).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(p.help_muted)
        };
        let label_style = Style::default().fg(if enabled { p.text } else { p.help_muted });
        let mut spans = vec![
            Span::styled(format!(" {:>5}", kind.key()), key_style),
            Span::styled(" → ", Style::default().fg(p.help_muted)),
            Span::styled(kind.label(), label_style),
        ];
        if let Some(reason) = reason {
            spans.push(Span::styled(
                format!("  ({reason})"),
                Style::default().fg(p.help_muted),
            ));
        }
        lines.push(Line::from(spans));
    }
    lines.push(Line::from(vec![
        Span::styled(
            " h/Esc",
            Style::default().fg(p.text).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" → ", Style::default().fg(p.help_muted)),
        Span::styled("back", Style::default().fg(p.text)),
    ]));
    let area = f.area();
    let rect = centered_rect(
        area,
        popup_width(&lines, area.width),
        lines.len() as u16 + 2,
    );
    render_popup(
        f,
        rect,
        Paragraph::new(lines).block(popup_block(Some("PR actions"), p)),
    );
}

pub fn render_pr_confirm(f: &mut Frame, app: &App) {
    let Some(confirm) = &app.pending_pr_confirm else {
        return;
    };
    let p = &app.palette;
    let plan = &confirm.plan;
    let mut lines: Vec<Line> = plan
        .lines
        .iter()
        .map(|l| Line::from(Span::styled(format!(" {l}"), Style::default().fg(p.text))))
        .collect();
    if let Some(w) = &plan.warning {
        lines.push(Line::from(Span::styled(
            format!(" Warning: {w}"),
            Style::default().fg(p.danger),
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        " Will run:",
        Style::default().fg(p.dimmed),
    )));
    for step in &plan.steps {
        lines.push(Line::from(Span::styled(
            format!("   $ {}", display_cmd(step)),
            Style::default().fg(p.accent),
        )));
    }
    if plan.remove_after {
        lines.push(Line::from(Span::styled(
            format!("   then: workmux remove {}", confirm.target.handle),
            Style::default().fg(p.accent),
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled(
            " y",
            Style::default().fg(p.text).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" confirm  ", Style::default().fg(p.dimmed)),
        Span::styled(
            "n",
            Style::default().fg(p.text).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" cancel", Style::default().fg(p.dimmed)),
    ]));
    let area = f.area();
    let rect = centered_rect(
        area,
        popup_width(&lines, area.width),
        lines.len() as u16 + 4,
    );
    render_popup(
        f,
        rect,
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(popup_block(Some(plan.title), p)),
    );
}
