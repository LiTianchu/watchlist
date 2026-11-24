use crate::config::WatchOptions;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub path: PathBuf,
    pub command: String,
    pub args: Vec<String>,
    #[serde(default)]
    pub options: WatchOptions,
}

impl Record {
    pub fn set_label(&mut self, value: &str) {
        let value = value.trim();
        self.label = (!value.is_empty()).then(|| value.to_owned());
    }

    pub fn display_name(&self) -> &str {
        self.label.as_deref().unwrap_or(&self.command)
    }

    pub fn from_line(line: &str) -> io::Result<Self> {
        let record: Self = if line.starts_with('{') {
            serde_json::from_str(line).map_err(invalid)?
        } else {
            let parts: Vec<_> = line.splitn(3, "\\0").collect();
            if parts.len() != 3 {
                return Err(invalid("expected a legacy record or JSON record"));
            }
            Self {
                version: 1,
                label: None,
                path: parts[0].into(),
                command: parts[1].into(),
                args: shell_words::split(parts[2]).map_err(invalid)?,
                options: WatchOptions::default(),
            }
        };
        record.validate()?;
        Ok(record)
    }

    fn validate(&self) -> io::Result<()> {
        if self.version != 1 {
            return Err(invalid("unsupported record version"));
        }
        if self.command.is_empty() || self.path.as_os_str().is_empty() {
            return Err(invalid("record needs a path and executable"));
        }
        self.options.validate().map_err(invalid)
    }

    pub fn to_line(&self) -> io::Result<String> {
        self.validate()?;
        serde_json::to_string(self).map_err(invalid)
    }
}

fn invalid(e: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, e.to_string())
}

pub fn resolve_save_path(name: impl Into<String>) -> io::Result<PathBuf> {
    if let Some(dir) = std::env::var_os("WATCHLIST_DATA_DIR") {
        let dir = PathBuf::from(dir);
        fs::create_dir_all(&dir)?;
        return Ok(dir.join(name.into()));
    }
    let dir = dirs::data_dir()
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "could not determine user data directory",
            )
        })?
        .join("watchlist");
    fs::create_dir_all(&dir)?;
    Ok(dir.join(name.into()))
}

/// The sidecar lock is stable across atomic replacement of the data file.
#[derive(Debug, Clone)]
pub struct Store {
    path: PathBuf,
}
impl Store {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    fn transaction<T>(
        &self,
        f: impl FnOnce(&mut Vec<String>) -> io::Result<(T, bool)>,
    ) -> io::Result<T> {
        let parent = self
            .path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        fs::create_dir_all(parent)?;
        let mut lock_name = self.path.as_os_str().to_os_string();
        lock_name.push(".lock");
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(PathBuf::from(lock_name))?;
        lock.lock()?;
        // All file handles to the destination close before replacement (also on Windows).
        let content = match fs::read_to_string(&self.path) {
            Ok(s) => s,
            Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(e),
        };
        let mut lines = content.lines().map(str::to_owned).collect();
        let (result, changed) = f(&mut lines)?;
        if changed {
            let mut temp = tempfile::NamedTempFile::new_in(parent)?;
            for line in lines {
                writeln!(temp, "{line}")?;
            }
            temp.as_file().sync_all()?;
            temp.persist(&self.path).map_err(|e| e.error)?;
            #[cfg(unix)]
            fs::File::open(parent)?.sync_all()?;
        }
        Ok(result)
    }

    pub fn lines(&self) -> io::Result<Vec<String>> {
        self.transaction(|lines| Ok((lines.clone(), false)))
    }
    pub fn records(&self) -> io::Result<Vec<Record>> {
        parse_records(&self.lines()?)
    }
    pub fn save(&self, record: &Record) -> io::Result<()> {
        self.transaction(|lines| {
            let mut records = parse_records(lines)?;
            if records.contains(record) {
                return Ok(((), false));
            }
            records.push(record.clone());
            *lines = records
                .iter()
                .map(Record::to_line)
                .collect::<io::Result<_>>()?;
            Ok(((), true))
        })
    }
    pub fn append_line(&self, line: String) -> io::Result<()> {
        if line.contains(['\n', '\r']) {
            return Err(invalid("record must occupy one line"));
        }
        self.transaction(|lines| {
            if lines.contains(&line) {
                return Ok(((), false));
            }
            lines.push(line);
            Ok(((), true))
        })
    }
    /// Optimistic comparison under the sidecar lock protects editors with stale indices.
    pub fn replace_record(
        &self,
        index: usize,
        expected: &Record,
        replacement: &Record,
    ) -> io::Result<()> {
        self.change_record(index, expected, Some(replacement))
    }
    pub fn delete_record(&self, index: usize, expected: &Record) -> io::Result<()> {
        self.change_record(index, expected, None)
    }
    fn change_record(
        &self,
        index: usize,
        expected: &Record,
        replacement: Option<&Record>,
    ) -> io::Result<()> {
        self.transaction(|lines| {
            let mut records = parse_records(lines)?;
            if records.get(index) != Some(expected) {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "record changed on disk; cancel editing and reload before trying again",
                ));
            }
            if let Some(record) = replacement {
                records[index] = record.clone();
            } else {
                records.remove(index);
            }
            *lines = records
                .iter()
                .map(Record::to_line)
                .collect::<io::Result<_>>()?;
            Ok(((), true))
        })
    }
    pub fn remove(&self, index: usize) -> io::Result<bool> {
        self.transaction(|lines| {
            if index >= lines.len() {
                return Ok((false, false));
            }
            lines.remove(index);
            Ok((true, true))
        })
    }
}
fn parse_records(lines: &[String]) -> io::Result<Vec<Record>> {
    lines
        .iter()
        .enumerate()
        .map(|(i, line)| Record::from_line(line).map_err(|e| invalid(format!("record {i}: {e}"))))
        .collect()
}

// Retained for existing library consumers.
pub fn read_save_lines(name: impl Into<String>) -> io::Result<Vec<String>> {
    Store::new(resolve_save_path(name)?).lines()
}
pub fn write_new_line(name: impl Into<String>, line: impl Into<String>) -> io::Result<()> {
    Store::new(resolve_save_path(name)?).append_line(line.into())
}
pub fn remove_line_by_index(name: impl Into<String>, index: usize) -> io::Result<()> {
    if !Store::new(resolve_save_path(name)?).remove(index)? {
        eprintln!("No record at index {index}");
    }
    Ok(())
}
pub fn read_line_by_index(name: impl Into<String>, index: usize) -> io::Result<String> {
    read_save_lines(name)?
        .get(index)
        .cloned()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "record index not found"))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn record(command: &str) -> Record {
        Record {
            version: 1,
            label: None,
            path: PathBuf::from("project"),
            command: command.into(),
            args: vec![
                "".into(),
                "hello world".into(),
                "'quoted'".into(),
                "\\0\n".into(),
            ],
            options: WatchOptions::default(),
        }
    }
    #[test]
    fn record_round_trip_and_zero_arguments() {
        let mut record = record("program");
        assert_eq!(
            Record::from_line(&record.to_line().unwrap()).unwrap(),
            record
        );
        record.args.clear();
        assert_eq!(
            Record::from_line(&record.to_line().unwrap()).unwrap(),
            record
        );
    }
    #[test]
    fn optional_labels_round_trip_and_old_records_remain_readable() {
        let mut record = record("program");
        assert!(!record.to_line().unwrap().contains("\"label\""));
        assert_eq!(
            Record::from_line(&record.to_line().unwrap()).unwrap().label,
            None
        );
        assert_eq!(Record::from_line("project\\0pwd\\0").unwrap().label, None);
        record.set_label("  Build 工具  ");
        assert_eq!(record.label.as_deref(), Some("Build 工具"));
        assert_eq!(
            Record::from_line(&record.to_line().unwrap()).unwrap(),
            record
        );
        record.set_label("   ");
        assert_eq!(record.label, None);
        assert_eq!(record.display_name(), "program");
        assert!(!record.to_line().unwrap().contains("\"label\""));
    }
    #[test]
    fn legacy_quoting_and_empty_args() {
        let record = Record::from_line("project\\0sh\\0-c 'printf hello'").unwrap();
        assert_eq!(record.args, ["-c", "printf hello"]);
        assert!(
            Record::from_line("project\\0pwd\\0")
                .unwrap()
                .args
                .is_empty()
        );
        assert_eq!(
            Record::from_line("project\\0printf\\0'hello world' ''")
                .unwrap()
                .args,
            ["hello world", ""]
        );
    }
    #[test]
    fn malformed_data_is_rejected() {
        for line in [
            "",
            "broken",
            "p\\0\\0",
            "p\\0sh\\0'unclosed",
            r#"{"version":20,"path":"p","command":"c","args":[]}"#,
        ] {
            assert!(Record::from_line(line).is_err(), "{line}");
        }
    }
    #[test]
    fn migration_and_nonexistent_delete_preserve_records() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("records");
        fs::write(&path, "project\\0pwd\\0\n").unwrap();
        let store = Store::new(&path);
        let before = fs::read(&path).unwrap();
        assert!(!store.remove(8).unwrap());
        assert_eq!(fs::read(&path).unwrap(), before);
        store.save(&record("program")).unwrap();
        let records = store.records().unwrap();
        assert!(records[0].args.is_empty());
        assert_eq!(records.len(), 2);
        assert!(fs::read_to_string(&path).unwrap().starts_with('{'));
    }
    #[test]
    fn malformed_store_is_never_overwritten_by_save() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("records");
        fs::write(&path, "broken\n").unwrap();
        assert!(Store::new(&path).save(&record("cmd")).is_err());
        assert_eq!(fs::read_to_string(path).unwrap(), "broken\n");
    }
    #[test]
    fn concurrent_updates_and_deduplication() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().join("records"));
        std::thread::scope(|scope| {
            for i in 0..12 {
                let store = &store;
                scope.spawn(move || {
                    for _ in 0..3 {
                        store.save(&record(&format!("cmd{i}"))).unwrap();
                    }
                });
            }
        });
        assert_eq!(store.records().unwrap().len(), 12);
        std::thread::scope(|scope| {
            for _ in 0..6 {
                let store = &store;
                scope.spawn(move || {
                    assert!(store.remove(0).unwrap());
                });
            }
            for i in 12..18 {
                let store = &store;
                scope.spawn(move || {
                    store.save(&record(&format!("cmd{i}"))).unwrap();
                });
            }
        });
        assert_eq!(store.records().unwrap().len(), 12);
    }
}
