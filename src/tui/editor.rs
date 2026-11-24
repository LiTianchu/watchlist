use crate::{
    config::{ExecutionPolicy, WatchOptions},
    saver::Record,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::{io, path::PathBuf};

pub const TABS: [&str; 4] = ["General", "Includes", "Excludes", "Runtime"];

pub struct Editor {
    pub draft: Record,
    pub original: Option<(usize, Record)>,
    baseline: Record,
    pub tab: usize,
    pub selected: [usize; 4],
}
impl Editor {
    pub fn new(record: Record, original: Option<(usize, Record)>) -> Self {
        Self {
            baseline: record.clone(),
            draft: record,
            original,
            tab: 0,
            selected: [0; 4],
        }
    }
    pub fn blank() -> io::Result<Self> {
        Ok(Self::new(
            Record {
                version: 1,
                label: None,
                path: std::env::current_dir()?,
                command: String::new(),
                args: vec![],
                options: WatchOptions::default(),
            },
            None,
        ))
    }
    pub fn dirty(&self) -> bool {
        self.draft != self.baseline
    }
    pub fn rows(&self) -> usize {
        match self.tab {
            0 => 5,
            1 => self.draft.options.includes.len(),
            2 => self.draft.options.excludes.len(),
            _ => 7,
        }
    }
    pub fn move_selection(&mut self, down: bool) {
        let max = self.rows().saturating_sub(1);
        let row = &mut self.selected[self.tab];
        *row = if down {
            row.saturating_add(1).min(max)
        } else {
            row.saturating_sub(1)
        };
    }
    pub fn selected_field(&self) -> Option<Field> {
        let row = self.selected[self.tab];
        match self.tab {
            0 => Some(
                [
                    Field::Path,
                    Field::Command,
                    Field::Args,
                    Field::Workdir,
                    Field::Label,
                ][row],
            ),
            1 if row < self.rows() => Some(Field::Include(Some(row))),
            2 if row < self.rows() => Some(Field::Exclude(Some(row))),
            3 if row < 4 => {
                Some([Field::Debounce, Field::MaxWait, Field::Poll, Field::Timeout][row])
            }
            _ => None,
        }
    }
    pub fn toggle(&mut self) {
        if self.tab != 3 {
            return;
        }
        let options = &mut self.draft.options;
        match self.selected[3] {
            4 => {
                options.policy = match options.policy {
                    ExecutionPolicy::Queue => ExecutionPolicy::Restart,
                    ExecutionPolicy::Restart => ExecutionPolicy::Ignore,
                    ExecutionPolicy::Ignore => ExecutionPolicy::Queue,
                }
            }
            5 => options.default_ignores = !options.default_ignores,
            6 => options.initial = !options.initial,
            _ => {}
        }
    }
    pub fn remove_pattern(&mut self) {
        let list = match self.tab {
            1 => &mut self.draft.options.includes,
            2 => &mut self.draft.options.excludes,
            _ => return,
        };
        let row = &mut self.selected[self.tab];
        if *row < list.len() {
            list.remove(*row);
            *row = (*row).min(list.len().saturating_sub(1));
        }
    }
    pub fn copy_defaults(&mut self) {
        let (list, defaults) = match self.tab {
            1 => (
                &mut self.draft.options.includes,
                crate::patterns::WATCH_PATTERNS,
            ),
            2 => {
                self.draft.options.default_ignores = false;
                (
                    &mut self.draft.options.excludes,
                    crate::patterns::IGNORE_PATTERNS,
                )
            }
            _ => return,
        };
        for pattern in defaults {
            if !list.iter().any(|existing| existing == pattern) {
                list.push((*pattern).into());
            }
        }
    }
    pub fn apply(&mut self, field: Field, value: &str) -> Result<(), String> {
        let mut candidate = self.draft.clone();
        match field {
            Field::Label => candidate.set_label(value),
            Field::Path => {
                if value.is_empty() {
                    return Err("Watch directory cannot be empty".into());
                }
                candidate.path = value.into();
            }
            Field::Command => {
                if value.is_empty() {
                    return Err("Executable cannot be empty".into());
                }
                candidate.command = value.into();
            }
            Field::Args => {
                candidate.args = shell_words::split(value)
                    .map_err(|e| format!("Invalid argument quoting: {e}"))?
            }
            Field::Workdir => {
                candidate.options.workdir = (!value.is_empty()).then(|| PathBuf::from(value))
            }
            Field::Debounce => candidate.options.debounce_ms = number(value)?,
            Field::MaxWait => candidate.options.max_wait_ms = number(value)?,
            Field::Poll => candidate.options.poll_ms = optional_number(value)?,
            Field::Timeout => candidate.options.timeout_ms = optional_number(value)?,
            Field::Include(index) | Field::Exclude(index) => {
                if value.trim().is_empty() {
                    return Err("Pattern cannot be empty".into());
                }
                glob::Pattern::new(value).map_err(|e| format!("Invalid glob: {e}"))?;
                let list = if matches!(field, Field::Include(_)) {
                    &mut candidate.options.includes
                } else {
                    &mut candidate.options.excludes
                };
                if let Some(index) = index {
                    list[index] = value.into();
                } else {
                    list.push(value.into());
                    self.selected[self.tab] = list.len() - 1;
                }
            }
        }
        candidate.options.validate()?;
        self.draft = candidate;
        Ok(())
    }
}
fn number(value: &str) -> Result<u64, String> {
    value
        .parse()
        .map_err(|_| "Enter milliseconds as a nonnegative integer".into())
}
fn optional_number(value: &str) -> Result<Option<u64>, String> {
    if value.is_empty() {
        Ok(None)
    } else {
        number(value).map(Some)
    }
}

#[derive(Clone, Copy)]
pub enum Field {
    Label,
    Path,
    Command,
    Args,
    Workdir,
    Debounce,
    MaxWait,
    Poll,
    Timeout,
    Include(Option<usize>),
    Exclude(Option<usize>),
}
impl Field {
    pub fn label(self) -> &'static str {
        match self {
            Self::Label => "Label (optional)",
            Self::Path => "Watch directory",
            Self::Command => "Executable",
            Self::Args => "Arguments",
            Self::Workdir => "Working directory",
            Self::Debounce => "Debounce (ms)",
            Self::MaxWait => "Maximum wait (ms)",
            Self::Poll => "Polling interval (ms)",
            Self::Timeout => "Timeout (ms)",
            Self::Include(_) => "Include pattern",
            Self::Exclude(_) => "Exclude pattern",
        }
    }
    pub fn hint(self) -> &'static str {
        match self {
            Self::Label => {
                "Optional nickname shown in saved records. Leave empty to remove the label."
            }
            Self::Path => {
                "Directory to watch. Relative paths resolve from the directory you launched Watchlist in."
            }
            Self::Command => {
                "Executable name or path, without quotes. Commands are never run by this editor."
            }
            Self::Args => {
                "Shell-style argument quoting: 'hello world' '' preserves spaces and an empty argument."
            }
            Self::Workdir => "Leave empty to run in the watched directory.",
            Self::Debounce => "Quiet period in milliseconds; 0 is allowed.",
            Self::MaxWait => "Maximum delay during continuous changes. Must be greater than 0.",
            Self::Poll => {
                "Leave empty for native events. Otherwise enter a positive interval in milliseconds."
            }
            Self::Timeout => "Leave empty for no timeout. Otherwise enter positive milliseconds.",
            Self::Include(_) => {
                "Example: **/*.rs. Custom includes replace the default extension allowlist."
            }
            Self::Exclude(_) => "Example: **/generated/**. Exclusions always win over includes.",
        }
    }
    pub fn value(self, record: &Record) -> String {
        match self {
            Self::Label => record.label.clone().unwrap_or_default(),
            Self::Path => record.path.to_string_lossy().into(),
            Self::Command => record.command.clone(),
            Self::Args => shell_words::join(&record.args),
            Self::Workdir => record
                .options
                .workdir
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default(),
            Self::Debounce => record.options.debounce_ms.to_string(),
            Self::MaxWait => record.options.max_wait_ms.to_string(),
            Self::Poll => record
                .options
                .poll_ms
                .map(|ms| ms.to_string())
                .unwrap_or_default(),
            Self::Timeout => record
                .options
                .timeout_ms
                .map(|ms| ms.to_string())
                .unwrap_or_default(),
            Self::Include(i) => i
                .map(|i| record.options.includes[i].clone())
                .unwrap_or_default(),
            Self::Exclude(i) => i
                .map(|i| record.options.excludes[i].clone())
                .unwrap_or_default(),
        }
    }
}

/// UTF-8 character-boundary editing; cursor is a byte offset into text.
pub struct Input {
    pub field: Field,
    pub text: String,
    pub cursor: usize,
}
impl Input {
    pub fn new(field: Field, record: &Record) -> Self {
        let text = field.value(record);
        let cursor = text.len();
        Self {
            field,
            text,
            cursor,
        }
    }
    pub fn insert(&mut self, text: &str) {
        let text: String = text
            .chars()
            .filter_map(|c| match c {
                '\n' | '\r' | '\t' => Some(' '),
                c if c.is_control() => None,
                c => Some(c),
            })
            .collect();
        self.text.insert_str(self.cursor, &text);
        self.cursor += text.len();
    }
    pub fn key(&mut self, key: KeyEvent) {
        let previous = |text: &str, cursor| {
            text[..cursor]
                .char_indices()
                .next_back()
                .map_or(0, |(i, _)| i)
        };
        let next = |text: &str, cursor: usize| {
            text[cursor..]
                .chars()
                .next()
                .map_or(cursor, |c| cursor + c.len_utf8())
        };
        match key.code {
            KeyCode::Left => self.cursor = previous(&self.text, self.cursor),
            KeyCode::Right => self.cursor = next(&self.text, self.cursor),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.text.len(),
            KeyCode::Backspace if self.cursor > 0 => {
                let start = previous(&self.text, self.cursor);
                self.text.drain(start..self.cursor);
                self.cursor = start;
            }
            KeyCode::Delete if self.cursor < self.text.len() => {
                self.text.drain(self.cursor..next(&self.text, self.cursor));
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.text.clear();
                self.cursor = 0;
            }
            KeyCode::Char(c)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.insert(&c.to_string())
            }
            _ => {}
        }
    }
}
