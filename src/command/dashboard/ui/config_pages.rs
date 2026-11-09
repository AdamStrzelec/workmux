//! Tools and Claude pages of the Project Config editor.

use ratatui::{
    Frame,
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Paragraph, Wrap},
};

use super::super::app::{
    App, ClaudeField, ConfigPage, SessionConfigEditor, ToolField, claude_field_text,
};
use super::help::render_modal_footer_row;
use super::popup::{centered_rect, popup_block, render_popup};
use super::theme::ThemePalette;

/// `[Layout] Tools Claude` indicator line.
pub fn page_tabs_line(palette: &ThemePalette, current: ConfigPage) -> Line<'static> {
    let mut spans = vec![Span::raw(" ")];
    for page in ConfigPage::ALL {
        let style = if page == current {
            Style::default()
                .fg(palette.accent)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            Style::default().fg(palette.dimmed)
        };
        spans.push(Span::styled(page.label().to_string(), style));
        spans.push(Span::raw("  "));
    }
    spans.push(Span::styled("(Tab)", Style::default().fg(palette.dimmed)));
    Line::from(spans)
}

pub fn render_config_page(f: &mut Frame, app: &App, editor: &SessionConfigEditor) {
    let palette = &app.palette;
    let dim = Style::default().fg(palette.dimmed);
    let mut lines: Vec<Line> = vec![
        Line::from(vec![
            Span::styled(
                format!(" {} ", editor.project_name),
                Style::default()
                    .fg(palette.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                editor.config_path.display().to_string(),
                Style::default().fg(palette.text),
            ),
        ]),
        page_tabs_line(palette, editor.page),
        Line::from(""),
    ];
    let row = |selected: bool| {
        if selected {
            Style::default().fg(palette.accent)
        } else {
            Style::default().fg(palette.text)
        }
    };

    match editor.page {
        ConfigPage::Tools => {
            let page = &editor.tools_page;
            lines.push(Line::from(vec![
                Span::styled(" Close tool windows with c: ", dim),
                Span::styled(
                    if page.tools.close_with_session() {
                        "yes"
                    } else {
                        "no"
                    },
                    Style::default()
                        .fg(palette.accent)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
            lines.push(Line::from(""));
            if page.tools.items.is_empty() {
                lines.push(Line::from(Span::styled(
                    " No tools yet. a adds one (key, name, command).",
                    dim,
                )));
            }
            let name_w = page
                .tools
                .items
                .iter()
                .map(|t| t.name.chars().count())
                .max()
                .unwrap_or(0)
                .clamp(4, 16);
            for (i, tool) in page.tools.items.iter().enumerate() {
                let selected = i == page.cursor;
                let style = row(selected);
                let editing = page.edit.as_ref().filter(|_| selected);
                let text = |field: ToolField, value: &str| match editing {
                    Some((f, buffer)) if *f == field => format!("{buffer}_"),
                    _ => value.to_string(),
                };
                let key = text(ToolField::Key, &tool.key);
                let key = if key.is_empty() { "?".into() } else { key };
                lines.push(Line::from(vec![
                    Span::styled(if selected { "> " } else { "  " }, style),
                    Span::styled(format!("{key:>3} "), style.add_modifier(Modifier::BOLD)),
                    Span::styled(
                        format!("{:<name_w$}  ", text(ToolField::Name, &tool.name)),
                        style.add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(text(ToolField::Command, &tool.command), style),
                ]));
            }
            lines.push(Line::from(""));
            if page.edit.is_some() {
                lines.push(render_modal_footer_row(
                    palette,
                    &[("Enter", " confirm  "), ("Esc", " cancel edit")],
                ));
            } else {
                lines.push(render_modal_footer_row(
                    palette,
                    &[
                        ("Enter", " command  "),
                        ("n", " name  "),
                        ("b", " key  "),
                        ("a", " add  "),
                        ("d", " delete  "),
                        ("w", " close with c"),
                    ],
                ));
            }
        }
        ConfigPage::Claude => {
            let page = editor.active_claude_page();
            let scope = match &editor.claude_worktree {
                Some(wt) if editor.claude_scope_worktree => {
                    format!(
                        " Scope: worktree '{}' (overrides project, w: project)",
                        wt.handle
                    )
                }
                Some(wt) => format!(" Scope: project (w: override for '{}')", wt.handle),
                None => " Scope: project".to_string(),
            };
            lines.push(Line::from(Span::styled(
                scope,
                Style::default().fg(palette.accent),
            )));
            lines.push(Line::from(""));
            for (i, field) in ClaudeField::ALL.into_iter().enumerate() {
                let selected = i == page.cursor;
                let style = row(selected);
                let value = match page.edit.as_ref().filter(|_| selected) {
                    Some(buffer) => format!("{buffer}_"),
                    None => {
                        let v = claude_field_text(&page.config, field);
                        if v.is_empty() { "(unset)".into() } else { v }
                    }
                };
                let mut spans = vec![
                    Span::styled(if selected { "> " } else { "  " }, style),
                    Span::styled(format!("{:<21} ", field.label()), dim),
                    Span::styled(value, style.add_modifier(Modifier::BOLD)),
                ];
                if selected && page.edit.is_none() {
                    let hint = match field {
                        ClaudeField::Model => "  Space: opus/sonnet/haiku, Enter: custom",
                        ClaudeField::PermissionMode => "  Space cycles",
                        ClaudeField::Preset => "  Space cycles",
                        ClaudeField::ResumeOnReopen => "  Space toggles",
                        ClaudeField::ExtraArgs => "  shell words",
                        ClaudeField::AppendSystemPrompt => "",
                        _ => "  comma separated",
                    };
                    spans.push(Span::styled(hint, dim));
                }
                lines.push(Line::from(spans));
            }
            lines.push(Line::from(""));
            let preview = editor.claude_args_preview();
            lines.push(Line::from(vec![
                Span::styled(" claude ", dim),
                Span::styled(
                    if preview.is_empty() {
                        "(no extra args)".to_string()
                    } else {
                        preview
                    },
                    Style::default().fg(palette.accent),
                ),
            ]));
            lines.push(Line::from(Span::styled(
                if editor.claude_scope_worktree {
                    " Worktree override: (unset) fields fall back to the project."
                } else {
                    " Applies to new claude agent panes; per-project settings."
                },
                dim,
            )));
            lines.push(Line::from(""));
            if page.edit.is_some() {
                lines.push(render_modal_footer_row(
                    palette,
                    &[("Enter", " confirm  "), ("Esc", " cancel edit")],
                ));
            } else {
                lines.push(render_modal_footer_row(
                    palette,
                    &[
                        ("Enter", " edit  "),
                        ("Space", " cycle  "),
                        ("d", " clear  "),
                        ("w", " project/worktree"),
                    ],
                ));
            }
        }
        ConfigPage::Layout => {}
    }
    lines.push(render_modal_footer_row(
        palette,
        &[
            (
                "s",
                if editor.is_dirty() {
                    " save*  "
                } else {
                    " save  "
                },
            ),
            ("Tab", " page  "),
            ("Esc", " cancel"),
        ],
    ));

    let content_width = lines.iter().map(Line::width).max().unwrap_or(40);
    let width = (content_width as u16 + 4).clamp(50, f.area().width.saturating_sub(4));
    let height = (lines.len() as u16 + 2).min(f.area().height);
    let area = centered_rect(f.area(), width, height);
    let block = popup_block(Some("Project Config"), palette);
    render_popup(
        f,
        area,
        Paragraph::new(Text::from(lines))
            .wrap(Wrap { trim: false })
            .block(block),
    );
}
