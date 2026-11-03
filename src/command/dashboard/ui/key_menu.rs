//! Space key menu: which-key style shortcut hints docked to the bottom.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use super::super::app::App;
use super::super::keymap::Context;
use super::popup::{popup_block, render_popup};

type Column = (&'static str, &'static [(&'static str, &'static str)]);

const AGENT_COLUMNS: &[Column] = &[
    (
        "Selected agent",
        &[
            ("Enter", "jump to agent"),
            ("p", "peek (keep dashboard)"),
            ("i", "input mode"),
            ("d", "view diff"),
            ("c", "commit changes"),
            ("m", "merge branch"),
            ("b", "change base branch"),
            ("o/O", "open PR / PR checks"),
            ("e", "open in VS Code"),
            ("X", "kill agent"),
            ("r", "remove worktree"),
        ],
    ),
    (
        "Navigation",
        &[
            ("j/k", "move down / up"),
            ("1-9", "quick jump"),
            ("Bksp", "last agent"),
            ("Tab", "worktrees tab"),
            ("C-u/C-d", "scroll preview"),
        ],
    ),
    (
        "View",
        &[
            ("/", "filter"),
            ("s", "cycle sort"),
            ("F", "session filter"),
            ("f", "stale filter"),
            ("+/-", "preview size"),
            ("T", "cycle theme"),
        ],
    ),
    (
        "General",
        &[
            (":", "command palette"),
            ("R", "sweep cleanup"),
            ("?", "full help"),
            ("Space", "close menu"),
            ("q", "quit"),
        ],
    ),
];

const WORKTREE_COLUMNS: &[Column] = &[
    (
        "Selected worktree",
        &[
            ("Enter", "open / jump to window"),
            ("c", "close window"),
            ("r", "remove worktree"),
            ("b", "change base branch"),
            ("o/O", "open PR / PR checks"),
            ("e", "open in VS Code"),
        ],
    ),
    (
        "Navigation",
        &[
            ("j/k", "move down / up"),
            ("1-9", "quick jump"),
            ("Tab", "agents tab"),
        ],
    ),
    (
        "View",
        &[
            ("/", "filter"),
            ("s", "cycle sort"),
            ("p", "switch project"),
            ("T", "cycle theme"),
        ],
    ),
    (
        "General",
        &[
            ("a", "add worktree"),
            (":", "command palette"),
            ("R", "sweep cleanup"),
            ("?", "full help"),
            ("Space", "close menu"),
            ("q", "quit"),
        ],
    ),
];

/// Columns shown for a keymap context; `None` when the menu does not apply.
pub(crate) fn key_menu_columns(ctx: Context) -> Option<&'static [Column]> {
    match ctx {
        Context::DashboardNormal => Some(AGENT_COLUMNS),
        Context::WorktreeNormal => Some(WORKTREE_COLUMNS),
        _ => None,
    }
}

const SEP: &str = " → ";
const COLUMN_GAP: usize = 3;

fn column_width((head, entries): &Column) -> (usize, usize) {
    let key_w = entries
        .iter()
        .map(|(k, _)| k.chars().count())
        .max()
        .unwrap_or(0);
    let body = entries
        .iter()
        .map(|(_, d)| key_w + SEP.chars().count() + d.chars().count())
        .max()
        .unwrap_or(0);
    (key_w, body.max(head.chars().count()) + COLUMN_GAP)
}

/// Greedy grid: columns side by side while they fit, wrap to a new band otherwise.
fn bands(widths: &[(usize, usize)], avail: usize) -> Vec<Vec<usize>> {
    let mut bands = Vec::new();
    let mut cur = Vec::new();
    let mut used = 0;
    for (i, (_, w)) in widths.iter().enumerate() {
        if !cur.is_empty() && used + w > avail {
            bands.push(std::mem::take(&mut cur));
            used = 0;
        }
        cur.push(i);
        used += w;
    }
    if !cur.is_empty() {
        bands.push(cur);
    }
    bands
}

pub fn render_key_menu(f: &mut Frame, app: &App) {
    let Some(columns) = key_menu_columns(app.keymap_context()) else {
        return;
    };
    let palette = &app.palette;
    let area = f.area();
    let inner_w = area.width.saturating_sub(4) as usize;
    let widths: Vec<_> = columns.iter().map(column_width).collect();

    let mut lines: Vec<Line> = Vec::new();
    for (n, band) in bands(&widths, inner_w).iter().enumerate() {
        if n > 0 {
            lines.push(Line::from(""));
        }
        let rows = band.iter().map(|&i| columns[i].1.len()).max().unwrap_or(0);
        for r in 0..=rows {
            let mut spans = vec![Span::raw(" ")];
            for &i in band {
                let (head, entries) = columns[i];
                let (key_w, col_w) = widths[i];
                let used = if r == 0 {
                    spans.push(Span::styled(
                        head,
                        Style::default()
                            .fg(palette.text)
                            .add_modifier(Modifier::BOLD),
                    ));
                    head.chars().count()
                } else if let Some((key, desc)) = entries.get(r - 1) {
                    spans.push(Span::styled(
                        format!("{key:>key_w$}"),
                        Style::default()
                            .fg(palette.dimmed)
                            .add_modifier(Modifier::BOLD),
                    ));
                    spans.push(Span::styled(SEP, Style::default().fg(palette.help_muted)));
                    spans.push(Span::styled(*desc, Style::default().fg(palette.text)));
                    key_w + SEP.chars().count() + desc.chars().count()
                } else {
                    0
                };
                spans.push(Span::raw(" ".repeat(col_w.saturating_sub(used))));
            }
            lines.push(Line::from(spans));
        }
    }

    let height = (lines.len() as u16 + 2).min(area.height);
    let rect = Rect::new(area.x, area.y + area.height - height, area.width, height);
    let block = popup_block(Some("Menu"), palette).title_bottom(Line::from(vec![
        Span::styled(" Space/Esc", Style::default().fg(palette.dimmed)),
        Span::styled(" close ", Style::default().fg(palette.help_muted)),
    ]));
    render_popup(f, rect, Paragraph::new(lines).block(block));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_only_in_normal_contexts() {
        assert!(key_menu_columns(Context::DashboardNormal).is_some());
        assert!(key_menu_columns(Context::WorktreeNormal).is_some());
        assert!(key_menu_columns(Context::DashboardInput).is_none());
        assert!(key_menu_columns(Context::WorktreeFilter).is_none());
    }

    #[test]
    fn bands_wrap_when_too_narrow() {
        let widths = [(1, 10), (1, 10), (1, 10)];
        assert_eq!(bands(&widths, 100), vec![vec![0, 1, 2]]);
        assert_eq!(bands(&widths, 25), vec![vec![0, 1], vec![2]]);
        assert_eq!(bands(&widths, 5), vec![vec![0], vec![1], vec![2]]);
    }
}
