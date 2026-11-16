//! Settings menu overlay: scalar options of global/project config.

use ratatui::{
    Frame,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use super::popup::{centered_rect, popup_block, render_popup};
use crate::command::dashboard::app::{App, SettingsOverlay};
use crate::settings_menu::{COMPLEX, OPTIONS};

pub fn render_settings_view(f: &mut Frame, app: &App) {
    let Some(menu) = &app.pending_settings else {
        return;
    };
    let p = &app.palette;
    let area = f.area();
    let width = area.width.saturating_sub(4).clamp(20, 110);
    let height = area.height.saturating_sub(2).max(8);
    let rect = centered_rect(area, width, height);

    let dim = Style::default().fg(p.dimmed);
    let mut rows: Vec<Line> = Vec::new();
    let mut cursor_line = 0usize;
    let mut last_section = "";
    for row in 0..SettingsOverlay::row_count() {
        let (key, section, shown, desc) = if let Some(o) = OPTIONS.get(row) {
            let value = menu.value_of(row);
            let shown = match (&value, &menu.edit) {
                (_, Some(buf)) if row == menu.cursor => format!("{buf}_"),
                (Some(v), _) => v.clone(),
                (None, _) => format!("{} (default)", o.default),
            };
            (o.key, o.section, shown, o.desc)
        } else {
            let (k, d) = COMPLEX[row - OPTIONS.len()];
            (k, "Nested (edit in file)", "-> e".to_string(), d)
        };
        if section != last_section {
            if !last_section.is_empty() {
                rows.push(Line::from(""));
            }
            rows.push(Line::from(Span::styled(
                section.to_string(),
                Style::default().fg(p.accent).add_modifier(Modifier::BOLD),
            )));
            last_section = section;
        }
        let selected = row == menu.cursor;
        if selected {
            cursor_line = rows.len();
        }
        let marker = if selected { "> " } else { "  " };
        let style = if selected {
            Style::default().add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        rows.push(Line::from(vec![
            Span::styled(format!("{marker}{key:<26}"), style),
            Span::styled(format!("{shown:<24}"), style),
            Span::styled(desc.to_string(), dim),
        ]));
    }

    // Keep the cursor row visible: drop lines from the top as needed.
    let body = height.saturating_sub(7) as usize;
    let skip = (cursor_line + 1).saturating_sub(body);
    let mut out: Vec<Line> = rows.into_iter().skip(skip).take(body).collect();
    out.push(Line::from(""));
    let scope_note = match menu.scope {
        crate::command::dashboard::app::SettingsScope::Global => {
            "global: personal, not shared".to_string()
        }
        crate::command::dashboard::app::SettingsScope::Project => {
            "project: edits .workmux.yaml (tracked by git, shared with others)".to_string()
        }
    };
    out.push(Line::from(vec![
        Span::styled(
            format!("Scope: {} ", menu.scope.label()),
            Style::default().fg(p.accent).add_modifier(Modifier::BOLD),
        ),
        Span::styled(scope_note, dim),
    ]));
    out.push(Line::from(Span::styled(
        menu.path().display().to_string(),
        dim,
    )));
    if let Some(m) = &menu.message {
        out.push(Line::from(m.clone()));
    } else {
        out.push(Line::from(""));
    }
    out.push(Line::from(Span::styled(
        "j/k move  Enter change  r reset  Tab scope  e editor  Esc close",
        dim,
    )));
    render_popup(
        f,
        rect,
        Paragraph::new(out).block(popup_block(Some("Settings"), p)),
    );
}
