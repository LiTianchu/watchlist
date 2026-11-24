use super::{
    App, Confirmation, Popup,
    editor::{Editor, Field, Input, TABS},
};
use crate::patterns::{IGNORE_PATTERNS, WATCH_PATTERNS};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Tabs, Wrap},
};
use unicode_width::UnicodeWidthStr;

const ACCENT: Color = Color::Cyan;
const MUTED: Color = Color::Gray;
const BG: Color = Color::Rgb(15, 23, 35);
fn panel(title: impl Into<Line<'static>>) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(Color::DarkGray))
}
fn highlight() -> Style {
    Style::default()
        .bg(Color::Rgb(30, 62, 84))
        .fg(Color::White)
        .add_modifier(Modifier::BOLD)
}
fn safe(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '\n' => '↵',
            '\t' => '→',
            c if c.is_control() => '�',
            c => c,
        })
        .collect()
}
fn dim(text: impl Into<String>) -> Line<'static> {
    Line::styled(text.into(), Style::default().fg(MUTED))
}
fn label(name: &str, value: impl AsRef<str>) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{name:<17}"), Style::default().fg(ACCENT)),
        Span::raw(safe(value.as_ref())),
    ])
}

pub(super) fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    frame.render_widget(
        Block::default().style(Style::default().bg(BG).fg(Color::White)),
        area,
    );
    if area.width < 60 || area.height < 18 {
        frame.render_widget(Paragraph::new("Watchlist configuration\n\nResize the terminal to at least 60 columns × 18 rows.\nEsc: back / quit    Ctrl+C: exit")
            .wrap(Wrap { trim: false }), area);
        return;
    }
    let layout = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(1),
        Constraint::Length(3),
    ])
    .split(area);
    let title = if let Some(editor) = &app.editor {
        format!(
            "WATCHLIST  /  {}{}",
            if editor.original.is_some() {
                "Edit configuration"
            } else {
                "New configuration"
            },
            if editor.dirty() { "  • unsaved" } else { "" }
        )
    } else {
        "WATCHLIST  /  Configuration manager".into()
    };
    frame.render_widget(
        Paragraph::new(Line::styled(
            title,
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ))
        .block(panel(" Saved records & watch rules ")),
        layout[0],
    );
    if let Some(editor) = &app.editor {
        draw_editor(frame, layout[1], editor);
    } else {
        draw_browser(frame, layout[1], app);
    }
    let keys = if app.editor.is_some() {
        "Tab: section  ↑↓: select  Enter: edit  a/d: add/delete rule  Ctrl+S: save  Esc: back  ?: help"
    } else {
        "↑↓: select  n: new  Enter/e: edit  c: copy  d: delete  r: reload  q: quit  ?: help"
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::styled(
                safe(&app.message),
                Style::default().fg(if app.error {
                    Color::LightRed
                } else {
                    Color::LightGreen
                }),
            ),
            dim(keys),
        ])
        .wrap(Wrap { trim: false }),
        layout[2],
    );
    if let Some(popup) = &app.popup {
        draw_popup(frame, area, popup, app);
    }
}

fn draw_browser(frame: &mut Frame, area: Rect, app: &mut App) {
    let columns =
        Layout::horizontal([Constraint::Percentage(36), Constraint::Percentage(64)]).split(area);
    let records: Vec<_> = app
        .records
        .iter()
        .enumerate()
        .map(|(i, record)| {
            ListItem::new(vec![
                Line::from(format!("{i}  {}", safe(record.display_name()))),
                dim(format!("   {}", safe(&record.path.display().to_string()))),
                Line::raw(""),
            ])
        })
        .collect();
    let list = List::new(records)
        .block(panel(format!(" Records ({}) ", app.records.len())))
        .highlight_style(highlight())
        .highlight_symbol("› ");
    frame.render_stateful_widget(list, columns[0], &mut app.list);
    if let Some((index, record)) = app.selected() {
        let o = &record.options;
        let includes = if o.includes.is_empty() {
            format!("Built-in extensions ({} rules)", WATCH_PATTERNS.len())
        } else {
            format!("{} custom rules", o.includes.len())
        };
        let mut lines = vec![
            label("Directory", record.path.display().to_string()),
            label("Executable", &record.command),
            label("Arguments", shell_words::join(&record.args)),
            label(
                "Working dir",
                o.workdir
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or("Watched directory".into()),
            ),
            Line::raw(""),
            label("Includes", includes),
            label("Excludes", format!("{} custom rules", o.excludes.len())),
            label(
                "Built-in ignores",
                if o.default_ignores {
                    "Enabled"
                } else {
                    "Disabled"
                },
            ),
            Line::raw(""),
            label(
                "Debounce / max",
                format!("{} / {} ms", o.debounce_ms, o.max_wait_ms),
            ),
            label(
                "Backend",
                o.poll_ms
                    .map(|ms| format!("Polling every {ms} ms"))
                    .unwrap_or("Native events".into()),
            ),
            label(
                "Timeout",
                o.timeout_ms
                    .map(|ms| format!("{ms} ms"))
                    .unwrap_or("Unlimited".into()),
            ),
            label("While running", format!("{:?}", o.policy)),
            label("Run on startup", if o.initial { "Yes" } else { "No" }),
            Line::raw(""),
            dim("Enter to edit this record. Changes are saved explicitly."),
            dim(format!("Run from your shell: watchlist --use {index}")),
        ];
        if !o.includes.is_empty() {
            lines.insert(6, dim(format!("  {}", safe(&o.includes.join(", ")))));
        }
        if let Some(name) = &record.label {
            lines.insert(0, label("Label", name));
        }
        frame.render_widget(
            Paragraph::new(lines)
                .block(panel(" Configuration details "))
                .wrap(Wrap { trim: false }),
            columns[1],
        );
    } else {
        frame.render_widget(
            Paragraph::new(vec![
                Line::raw(""),
                Line::styled(
                    "Your first watch configuration",
                    Style::default().fg(ACCENT).bold(),
                ),
                Line::raw(""),
                Line::raw("Press n to choose a directory and command."),
                Line::raw(""),
                dim("Then add includes and excludes, or keep the built-in rules."),
                Line::raw(""),
                dim("The editor manages configurations; it does not execute commands."),
            ])
            .block(panel(" Get started "))
            .wrap(Wrap { trim: false }),
            columns[1],
        );
    }
}

fn draw_editor(frame: &mut Frame, area: Rect, editor: &Editor) {
    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(2),
        Constraint::Min(1),
    ])
    .split(area);
    frame.render_widget(
        Tabs::new(
            TABS.iter()
                .enumerate()
                .map(|(i, name)| format!("{} {name}", i + 1)),
        )
        .select(editor.tab)
        .highlight_style(Style::default().fg(ACCENT).bold())
        .block(panel(" Sections · Tab / Shift+Tab ")),
        chunks[0],
    );
    let hint = match editor.tab {
        0 => {
            "Enter edits the selected field. Arguments use shell-style quoting. Commands are never executed here."
        }
        1 => {
            "a: add  Enter/e: edit  d: delete  p: copy defaults. Empty list uses built-in extensions. Exclusions win."
        }
        2 => {
            "a: add  Enter/e: edit  d: delete  p: copy built-in ignores here to customize them (disables automatic ignores)."
        }
        _ => {
            "Enter / Space edits a value or cycles a switch. Empty polling = native events; empty timeout = unlimited."
        }
    };
    frame.render_widget(
        Paragraph::new(hint)
            .style(Style::default().fg(MUTED))
            .wrap(Wrap { trim: false }),
        chunks[1],
    );
    let mut state = ListState::default().with_selected(Some(editor.selected[editor.tab]));
    match editor.tab {
        0 | 3 => {
            let fields = if editor.tab == 0 {
                vec![
                    Field::Path,
                    Field::Command,
                    Field::Args,
                    Field::Workdir,
                    Field::Label,
                ]
            } else {
                vec![Field::Debounce, Field::MaxWait, Field::Poll, Field::Timeout]
            };
            let mut items: Vec<ListItem> = fields
                .iter()
                .map(|field| {
                    let value = field.value(&editor.draft);
                    ListItem::new(vec![
                        Line::styled(field.label(), Style::default().fg(ACCENT)),
                        Line::raw(if value.is_empty() {
                            match field {
                                Field::Command => "<required>",
                                Field::Workdir => "<watched directory>",
                                Field::Poll => "<native events>",
                                Field::Timeout => "<unlimited>",
                                Field::Label => "<optional>",
                                _ => "<none>",
                            }
                            .into()
                        } else {
                            safe(&value)
                        }),
                        Line::raw(""),
                    ])
                })
                .collect();
            if editor.tab == 3 {
                let o = &editor.draft.options;
                for (name, value) in [
                    ("Execution policy", format!("{:?}", o.policy)),
                    (
                        "Built-in ignores",
                        if o.default_ignores { "On" } else { "Off" }.into(),
                    ),
                    (
                        "Run on startup",
                        if o.initial { "On" } else { "Off" }.into(),
                    ),
                ] {
                    items.push(ListItem::new(vec![
                        Line::styled(name, Style::default().fg(ACCENT)),
                        Line::raw(value),
                        Line::raw(""),
                    ]));
                }
            }
            frame.render_stateful_widget(
                List::new(items)
                    .highlight_style(highlight())
                    .highlight_symbol("› ")
                    .block(panel(" ↑↓ Select a field · Enter to change ")),
                chunks[2],
                &mut state,
            );
        }
        _ => {
            let list = if editor.tab == 1 {
                &editor.draft.options.includes
            } else {
                &editor.draft.options.excludes
            };
            let o = &editor.draft.options;
            let default_state = if editor.tab == 1 {
                if list.is_empty() {
                    format!(
                        "Using {} built-in extension rules. Press p to copy them into this list.",
                        WATCH_PATTERNS.len()
                    )
                } else {
                    "Custom includes replace the built-in extension allowlist.".into()
                }
            } else if o.default_ignores {
                format!(
                    "{} built-in ignores also apply. Press p to copy them here for editing.",
                    IGNORE_PATTERNS.len()
                )
            } else {
                "Built-in ignores are disabled; only the exclusions below apply.".into()
            };
            let rows =
                Layout::vertical([Constraint::Length(2), Constraint::Min(1)]).split(chunks[2]);
            frame.render_widget(
                Paragraph::new(default_state)
                    .style(Style::default().fg(Color::Yellow))
                    .wrap(Wrap { trim: false }),
                rows[0],
            );
            if list.is_empty() {
                frame.render_widget(Paragraph::new("\nNo custom rules.\n\nPress a to add a glob pattern, or p to copy the defaults.")
                    .block(panel(format!(" {} · 0 custom rules ", TABS[editor.tab]))).wrap(Wrap { trim: false }), rows[1]);
            } else {
                frame.render_stateful_widget(
                    List::new(
                        list.iter()
                            .enumerate()
                            .map(|(i, p)| ListItem::new(format!("{}  {}", i + 1, safe(p)))),
                    )
                    .highlight_style(highlight())
                    .highlight_symbol("› ")
                    .block(panel(format!(
                        " {} · {} custom rules ",
                        TABS[editor.tab],
                        list.len()
                    ))),
                    rows[1],
                    &mut state,
                );
            }
        }
    }
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width.saturating_sub(4));
    let height = height.min(area.height.saturating_sub(2));
    Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    )
}
fn draw_popup(frame: &mut Frame, area: Rect, popup: &Popup, app: &App) {
    match popup {
        Popup::Input(input) => draw_input(frame, area, input, app),
        Popup::Confirm(action) => {
            let (title, message) = match action {
                Confirmation::Delete(index, record) => (
                    " Delete saved record? ",
                    format!(
                        "Record {index}: {}\n{}\n\nThis removes the saved configuration.\n\n[y] Delete    [n / Esc] Keep record",
                        safe(record.display_name()),
                        safe(&record.path.display().to_string())
                    ),
                ),
                Confirmation::Discard { quit } => (
                    " Unsaved changes ",
                    format!(
                        "Discard this draft{}?\n\n[y] Discard    [n / Esc] Keep editing",
                        if *quit { " and quit" } else { "" }
                    ),
                ),
            };
            let rect = centered(area, 66, 10);
            frame.render_widget(Clear, rect);
            frame.render_widget(
                Paragraph::new(message)
                    .block(panel(title).border_style(Style::default().fg(Color::Yellow)))
                    .style(Style::default().bg(BG))
                    .wrap(Wrap { trim: false }),
                rect,
            );
        }
        Popup::Help => {
            let rect = centered(area, 78, 22);
            frame.render_widget(Clear, rect);
            frame.render_widget(Paragraph::new("BROWSER\n  ↑↓ / j k   Select record     Enter / e   Edit\n  n   New     c   Copy     d / Delete   Delete (confirmed)\n  r   Reload from disk     q / Esc   Quit\n\nEDITOR\n  Tab / Shift+Tab or 1–4   Switch sections\n  ↑↓   Select     Enter / e   Edit     Space   Toggle\n  a   Add pattern     d   Delete pattern     p   Copy defaults\n  Ctrl+S   Save draft     Esc   Back (confirms unsaved changes)\n\nTEXT INPUT\n  ←→ / Home / End   Move cursor     Ctrl+U   Clear field\n  Enter   Apply to draft     Esc   Cancel field edit\n  Bracketed paste is supported.\n\nExclusions win. Empty includes use the original extension rules.\nCopy built-in excludes with p to edit them individually.\n\nAny key closes help.")
                .block(panel(" Keyboard shortcuts ").border_style(Style::default().fg(ACCENT))).style(Style::default().bg(BG)).wrap(Wrap { trim: false }), rect);
        }
    }
}
fn draw_input(frame: &mut Frame, area: Rect, input: &Input, app: &App) {
    let rect = centered(area, 84, 12);
    frame.render_widget(Clear, rect);
    let block = panel(format!(" {} ", input.field.label()))
        .border_style(Style::default().fg(ACCENT))
        .style(Style::default().bg(BG));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    let rows = Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(3),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .split(inner);
    frame.render_widget(
        Paragraph::new(input.field.hint())
            .style(Style::default().fg(MUTED))
            .wrap(Wrap { trim: false }),
        rows[0],
    );
    let value = safe(&input.text);
    let prefix = safe(&input.text[..input.cursor]);
    let columns = rows[1].width.saturating_sub(2) as usize;
    let cursor = prefix.width();
    let scroll = cursor.saturating_sub(columns.saturating_sub(1));
    frame.render_widget(
        Paragraph::new(value)
            .block(panel(" Value "))
            .scroll((0, scroll.min(u16::MAX as usize) as u16)),
        rows[1],
    );
    frame.set_cursor_position((
        rows[1].x + 1 + cursor.saturating_sub(scroll) as u16,
        rows[1].y + 1,
    ));
    if app.error {
        frame.render_widget(
            Paragraph::new(safe(&app.message))
                .style(Style::default().fg(Color::LightRed))
                .wrap(Wrap { trim: false }),
            rows[2],
        );
    }
    frame.render_widget(
        Paragraph::new("Enter: apply   Esc: cancel   Ctrl+U: clear   ←→: cursor")
            .style(Style::default().fg(MUTED)),
        rows[3],
    );
}
