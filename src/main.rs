use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{Receiver, channel};
use std::thread;
use std::time::{Duration, Instant};

struct PendingEvent {
    last_seen: Instant,
    event: Event,
}

pub struct DebouncedWatcher {
    watcher: RecommendedWatcher,
}

impl DebouncedWatcher {
    pub fn new(
        watch_path: impl Into<String> + Send + 'static,
        debounce_duration: Duration,
        callback: impl Fn(Vec<Event>, String) + Send + 'static,
    ) -> notify::Result<Self> {
        let (raw_tx, raw_rx) = channel::<Event>();

        let watcher = RecommendedWatcher::new(
            move |result: Result<Event, _>| {
                if let Ok(event) = result {
                    let _ = raw_tx.send(event);
                }
            },
            Config::default(),
        )?;

        let path = watch_path.into();
        thread::spawn(move || {
            Self::debounce_loop(path, raw_rx, debounce_duration, callback);
        });

        Ok(Self { watcher: watcher })
    }

    fn debounce_loop(
        root_path: impl Into<String>,
        rx: Receiver<Event>,
        debounce_duration: Duration,
        callback: impl Fn(Vec<Event>, String),
    ) {
        let mut pending: HashMap<PathBuf, PendingEvent> = HashMap::new();
        let check_interval = Duration::from_millis(50);
        let path = root_path.into();

        loop {
            while let Ok(event) = rx.try_recv() {
                let now = Instant::now();

                for path in &event.paths {
                    pending
                        .entry(path.clone())
                        .and_modify(|p| {
                            p.last_seen = now;
                            p.event = event.clone();
                        })
                        .or_insert(PendingEvent {
                            last_seen: now,
                            event: event.clone(),
                        });
                }
            }
            let now = Instant::now();
            let mut ready_events: Vec<Event> = Vec::new();
            pending.retain(|_, pending_event| {
                if now.duration_since(pending_event.last_seen) >= debounce_duration {
                    ready_events.push(pending_event.event.clone());
                    false // remove from pending
                } else {
                    true // keep waiting
                }
            });

            if !ready_events.is_empty() {
                callback(ready_events, path.clone());
            }
            thread::sleep(check_interval)
        }
    }

    pub fn watch(&mut self, path: &Path, mode: RecursiveMode) -> notify::Result<()> {
        self.watcher.watch(path, mode)
    }
}

fn main() -> notify::Result<()> {
    let args: Vec<String> = env::args().collect();
    let path: String = args
        .iter()
        .position(|arg| arg == "--path")
        .map(|p| args.get(p + 1))
        .expect("No --path argument provided")
        .expect("Path argument is not valid")
        .clone();

    let mut debouncer =
        DebouncedWatcher::new(path.clone(), Duration::from_millis(500), debounce_callback)
            .expect("Debouncer failed to initialize.");

    debouncer.watch(Path::new(&path), RecursiveMode::Recursive)?;

    println!("Watching {:?} for changes...", path);

    // block the main thread to prevent the program from shutting down
    let (_tx, rx) = channel::<()>();
    rx.recv().ok();
    Ok(())
}

fn should_rebuild(event_kind: &EventKind) -> bool {
    match event_kind {
        EventKind::Modify(_) => true,
        EventKind::Create(_) => true,
        EventKind::Remove(_) => true,
        _ => false,
    }
}

fn debounce_callback(events: Vec<Event>, root_path: String) {
    let mut event_count = 0;

    let ignore_patterns = vec![
        glob::Pattern::new("**/target/**").unwrap(),
        glob::Pattern::new("**/.git/**").unwrap(),
        glob::Pattern::new("**/node_modules/**").unwrap(),
    ];
    let watch_patterns = vec![
        glob::Pattern::new("*.rs").unwrap(),
        glob::Pattern::new("*.toml").unwrap(),
    ];

    for event in events {
        let event_kind = event.kind;

        if !should_rebuild(&event_kind) {
            continue;
        }

        for path in event.paths {
            if ignore_patterns
                .iter()
                .any(|p| p.matches(&path.to_string_lossy()))
            {
                continue;
            }

            if !watch_patterns
                .iter()
                .any(|p| p.matches(&path.to_string_lossy()))
            {
                continue;
            }
            println!("path: {:?}, kind: {:?}", path, event.kind);

            event_count += 1;
        }
    }

    if event_count > 0 {
        let toml_path = [root_path.clone(), "/Cargo.toml".to_string()].concat();
        let target_path = [root_path.clone(), "/target".to_string()].concat();
        let status = Command::new("cargo")
            .args([
                "build",
                "--manifest-path",
                toml_path.as_ref(),
                "--target-dir",
                target_path.as_ref(),
            ])
            .status()
            .expect(&format!("failed to run cargo build on {:?}", root_path));
        if status.success() {
            println!("Build cargo project succeeded: {:?}", root_path);
        } else {
            eprintln!("Build failed: {:?}", root_path);
        }
    }
}
