//! TUI rendering logic for the dashboard.

mod config_pages;
mod dashboard;
mod diff;
mod error_modal;
mod format;
mod help;
mod key_menu;
mod popup;
mod pr_menu;
pub mod theme;
mod tool_menu;
pub mod worktree;

use ratatui::Frame;
use ratatui::style::{Modifier, Style};

use super::app::{App, ViewMode};

/// Dim all cells in the buffer to create a backdrop effect behind modals.
fn dim_buffer(f: &mut Frame) {
    let area = f.area();
    let buf = f.buffer_mut();
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            if let Some(cell) = buf.cell_mut((x, y)) {
                cell.set_style(Style::default().add_modifier(Modifier::DIM));
            }
        }
    }
}

pub use self::dashboard::render_dashboard;
pub use self::diff::render_diff_view;
pub use self::help::{
    render_add_worktree, render_base_picker, render_command_palette, render_confirm_kill,
    render_confirm_remove, render_help, render_project_picker, render_row_context,
    render_session_config, render_sweep, render_sweep_progress,
};
pub use self::key_menu::render_key_menu;
use self::pr_menu::{render_pr_confirm, render_pr_menu};
use self::tool_menu::render_tool_menu;

/// Main UI entry point - renders the appropriate view based on app state.
pub fn ui(f: &mut Frame, app: &mut App) {
    app.terminal_area = f.area();
    error_modal::promote_status_error(app);

    // Render either dashboard or diff view based on view mode
    match &mut app.view_mode {
        ViewMode::Dashboard => render_dashboard(f, app),
        ViewMode::Diff(diff_view) => render_diff_view(f, diff_view, &app.palette),
    }

    // Render overlays on top
    let has_modal = app.sweep_progress.is_some()
        || app.show_help
        || app.show_key_menu
        || app.pending_kill_pane_id.is_some()
        || app.pending_remove.is_some()
        || app.pending_pr_menu.is_some()
        || app.pending_tool_menu.is_some()
        || app.pending_pr_confirm.is_some()
        || app.pending_base_picker.is_some()
        || app.pending_project_picker.is_some()
        || app.pending_session_config.is_some()
        || app.pending_sweep.is_some()
        || app.pending_add_worktree.is_some()
        || app.pending_command_palette.is_some()
        || app.pending_row_context.is_some()
        || app.error_modal.is_some();

    if has_modal {
        dim_buffer(f);
    }

    if app.sweep_progress.is_some() {
        render_sweep_progress(f, app);
    } else if app.show_help {
        render_help(f, app);
    } else if app.show_key_menu {
        render_key_menu(f, app);
    } else if app.pending_kill_pane_id.is_some() {
        render_confirm_kill(f, app);
    } else if app.pending_remove.is_some() {
        render_confirm_remove(f, app);
    } else if app.pending_pr_confirm.is_some() {
        render_pr_confirm(f, app);
    } else if app.pending_pr_menu.is_some() {
        render_pr_menu(f, app);
    } else if app.pending_tool_menu.is_some() {
        render_tool_menu(f, app);
    } else if app.pending_row_context.is_some() {
        render_row_context(f, app);
    } else if app.pending_command_palette.is_some() {
        render_command_palette(f, app);
    } else if app.pending_base_picker.is_some() {
        render_base_picker(f, app);
    } else if app.pending_project_picker.is_some() {
        render_project_picker(f, app);
    } else if app.pending_session_config.is_some() {
        render_session_config(f, app);
    } else if app.pending_sweep.is_some() {
        render_sweep(f, app);
    } else if app.pending_add_worktree.is_some() {
        render_add_worktree(f, app);
    }

    // Errors stay on top of everything until dismissed.
    if app.error_modal.is_some() {
        error_modal::render_error_modal(f, app);
    }
}
