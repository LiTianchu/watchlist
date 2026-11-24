mod editor;
mod view;

use crate::saver::{Record, Store};
use crossterm::{
    event::{
        self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEvent, KeyEventKind,
        KeyModifiers,
    },
    execute,
};
use editor::{Editor, Field, Input};
use ratatui::widgets::ListState;
use std::{
    io::{self, IsTerminal},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

pub(super) enum Confirmation {
    Delete(usize, Box<Record>),
    Discard { quit: bool },
}
pub(super) enum Popup {
    Input(Input),
    Confirm(Confirmation),
    Help,
}
pub(super) struct App {
    store: Store,
    records: Vec<Record>,
    list: ListState,
    editor: Option<Editor>,
    popup: Option<Popup>,
    message: String,
    error: bool,
    quit: bool,
}
impl App {
    fn new(store: Store, selected: usize) -> io::Result<Self> {
        let records = store.records()?;
        let selected = if records.is_empty() {
            None
        } else {
            Some(selected.min(records.len() - 1))
        };
        Ok(Self {
            store,
            records,
            list: ListState::default().with_selected(selected),
            editor: None,
            popup: None,
            message: "Select a record to inspect it, or press n to create one.".into(),
            error: false,
            quit: false,
        })
    }
    fn status(&mut self, message: impl Into<String>, error: bool) {
        self.message = message.into();
        self.error = error;
    }
    fn selected(&self) -> Option<(usize, Record)> {
        let index = self.list.selected()?;
        self.records.get(index).cloned().map(|r| (index, r))
    }
    fn reload(&mut self) -> io::Result<()> {
        self.records = self.store.records()?;
        self.list.select(if self.records.is_empty() {
            None
        } else {
            Some(
                self.list
                    .selected()
                    .unwrap_or(0)
                    .min(self.records.len() - 1),
            )
        });
        Ok(())
    }
    fn request_leave(&mut self, quit: bool) {
        if self.editor.as_ref().is_some_and(Editor::dirty) {
            self.popup = Some(Popup::Confirm(Confirmation::Discard { quit }));
        } else {
            self.editor = None;
            self.quit = quit;
        }
    }
    fn save(&mut self) -> io::Result<()> {
        let Some(editor) = &self.editor else {
            return Ok(());
        };
        editor.draft.to_line()?;
        if let Some((index, original)) = &editor.original {
            self.store.replace_record(*index, original, &editor.draft)?;
        } else {
            self.store.save(&editor.draft)?;
        }
        let saved = editor.draft.clone();
        self.reload()?;
        self.list.select(
            self.records
                .iter()
                .position(|record| *record == saved)
                .or(self.list.selected()),
        );
        self.editor = None;
        self.status("Configuration saved. Use --use INDEX to watch it.", false);
        Ok(())
    }
    fn handle(&mut self, key: KeyEvent) {
        if key.kind == KeyEventKind::Release {
            return;
        }
        let result = self.key(key);
        if let Err(error) = result {
            self.status(error.to_string(), true);
        }
    }
    fn key(&mut self, key: KeyEvent) -> io::Result<()> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl && key.code == KeyCode::Char('c') {
            if matches!(self.popup, Some(Popup::Input(_))) {
                self.status(
                    "Finish the field with Enter, or cancel with Esc, before quitting.",
                    false,
                );
                return Ok(());
            }
            if self.popup.is_some() {
                self.popup = None;
            }
            self.request_leave(true);
            return Ok(());
        }
        if let Some(popup) = self.popup.take() {
            match popup {
                Popup::Input(mut input) => match key.code {
                    KeyCode::Esc => self.status("Field edit cancelled.", false),
                    KeyCode::Enter => {
                        match self
                            .editor
                            .as_mut()
                            .expect("input has an editor")
                            .apply(input.field, &input.text)
                        {
                            Ok(()) => self.status("Draft updated. Ctrl+S saves the record.", false),
                            Err(error) => {
                                self.popup = Some(Popup::Input(input));
                                self.status(error, true);
                            }
                        }
                    }
                    _ => {
                        input.key(key);
                        self.popup = Some(Popup::Input(input));
                    }
                },
                Popup::Confirm(action) => match key.code {
                    KeyCode::Char('y' | 'Y') => match action {
                        Confirmation::Delete(index, record) => {
                            self.store.delete_record(index, &record)?;
                            self.reload()?;
                            self.status("Record deleted.", false);
                        }
                        Confirmation::Discard { quit } => {
                            self.editor = None;
                            self.quit = quit;
                            self.status("Draft discarded.", false);
                        }
                    },
                    KeyCode::Esc | KeyCode::Char('n' | 'N') => {}
                    _ => self.popup = Some(Popup::Confirm(action)),
                },
                Popup::Help => {}
            }
            return Ok(());
        }
        if key.code == KeyCode::Char('?') {
            self.popup = Some(Popup::Help);
            return Ok(());
        }
        if ctrl && key.code == KeyCode::Char('s') {
            return self.save();
        }
        if self.editor.is_some() {
            if key.code == KeyCode::Esc {
                self.request_leave(false);
                return Ok(());
            }
            let editor = self.editor.as_mut().unwrap();
            match key.code {
                KeyCode::Tab | KeyCode::Right => editor.tab = (editor.tab + 1) % 4,
                KeyCode::BackTab | KeyCode::Left => editor.tab = (editor.tab + 3) % 4,
                KeyCode::Char(c @ '1'..='4') => editor.tab = c as usize - '1' as usize,
                KeyCode::Up | KeyCode::Char('k') => editor.move_selection(false),
                KeyCode::Down | KeyCode::Char('j') => editor.move_selection(true),
                KeyCode::Char('a') if matches!(editor.tab, 1 | 2) => {
                    let field = if editor.tab == 1 {
                        Field::Include(None)
                    } else {
                        Field::Exclude(None)
                    };
                    self.popup = Some(Popup::Input(Input::new(field, &editor.draft)));
                }
                KeyCode::Delete | KeyCode::Char('d') if matches!(editor.tab, 1 | 2) => {
                    editor.remove_pattern()
                }
                KeyCode::Char('p') if matches!(editor.tab, 1 | 2) => {
                    editor.copy_defaults();
                    self.status("Defaults copied into this draft. Edit individual rules, then Ctrl+S to save.", false);
                }
                KeyCode::Enter | KeyCode::Char('e' | ' ') => {
                    if let Some(field) = editor.selected_field() {
                        self.popup = Some(Popup::Input(Input::new(field, &editor.draft)));
                    } else if matches!(editor.tab, 1 | 2) {
                        let field = if editor.tab == 1 {
                            Field::Include(None)
                        } else {
                            Field::Exclude(None)
                        };
                        self.popup = Some(Popup::Input(Input::new(field, &editor.draft)));
                    } else {
                        editor.toggle();
                    }
                }
                _ => {}
            }
            return Ok(());
        }
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.quit = true,
            KeyCode::Up | KeyCode::Char('k') => {
                if let Some(index) = self.list.selected() {
                    self.list.select(Some(index.saturating_sub(1)));
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if let Some(index) = self.list.selected() {
                    self.list
                        .select(Some((index + 1).min(self.records.len() - 1)));
                }
            }
            KeyCode::Char('n') => {
                self.editor = Some(Editor::blank()?);
                self.status("New record. Enter edits a field; Ctrl+S saves.", false);
            }
            KeyCode::Enter | KeyCode::Char('e') => {
                if let Some((index, record)) = self.selected() {
                    self.editor = Some(Editor::new(record.clone(), Some((index, record))));
                    self.status("Editing a draft. Ctrl+S saves; Esc goes back.", false);
                }
            }
            KeyCode::Char('c') => {
                if let Some((_, record)) = self.selected() {
                    self.editor = Some(Editor::new(record, None));
                    self.status("Copy of record. Change fields and Ctrl+S to save.", false);
                }
            }
            KeyCode::Delete | KeyCode::Char('d') => {
                if let Some((index, record)) = self.selected() {
                    self.popup = Some(Popup::Confirm(Confirmation::Delete(
                        index,
                        Box::new(record),
                    )));
                }
            }
            KeyCode::Char('r') => {
                self.reload()?;
                self.status("Reloaded saved configurations.", false);
            }
            _ => {}
        }
        Ok(())
    }
}

struct TerminalGuard;
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = execute!(io::stdout(), DisableBracketedPaste);
        ratatui::restore();
    }
}

pub fn run(store: Store, selected: usize) -> io::Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(io::Error::other(
            "--tui needs an interactive terminal; use --list for noninteractive output",
        ));
    }
    // Load before entering raw mode, so malformed history never traps the terminal.
    let mut app = App::new(store, selected)?;
    let stop = Arc::new(AtomicBool::new(false));
    let signal_stop = stop.clone();
    ctrlc::set_handler(move || signal_stop.store(true, Ordering::SeqCst))
        .map_err(io::Error::other)?;
    let _guard = TerminalGuard;
    let mut terminal = ratatui::try_init()?;
    execute!(io::stdout(), EnableBracketedPaste)?;
    while !app.quit && !stop.load(Ordering::SeqCst) {
        terminal.draw(|frame| view::draw(frame, &mut app))?;
        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                Event::Key(key) => app.handle(key),
                Event::Paste(text) => {
                    if let Some(Popup::Input(input)) = &mut app.popup {
                        input.insert(&text);
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
