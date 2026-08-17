use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{Receiver, channel};
use std::thread;
use std::time::{Duration, Instant};

type EventAllowedPredicate = fn(&EventKind) -> bool;

#[derive(Debug, Clone)]
pub struct StatusMessages {
    pub panic_message: String,
    pub success_message: String,
    pub fail_message: String,
}

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
        command: String,
        command_args: Vec<String>,
        event_allowed: EventAllowedPredicate,
        status_messages: StatusMessages,
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

        thread::spawn(move || {
            let path_str = watch_path.into();
            let command_dir = Path::new(&path_str);

            let arg_refs = command_args
                .iter()
                .map(|arg| arg.as_str())
                .collect::<Vec<_>>();
            Self::debounce_loop(
                raw_rx,
                debounce_duration,
                command_exec_callback,
                &command_dir,
                &command,
                &arg_refs,
                event_allowed,
                status_messages,
            );
        });

        Ok(Self { watcher: watcher })
    }

    fn debounce_loop(
        rx: Receiver<Event>,
        debounce_duration: Duration,
        callback: impl Fn(Vec<Event>, &Path, &str, &[&str], EventAllowedPredicate, StatusMessages),
        command_dir: &Path,
        command: &str,
        command_args: &[&str],
        event_allowed: EventAllowedPredicate,
        status_messages: StatusMessages,
    ) {
        let mut pending: HashMap<PathBuf, PendingEvent> = HashMap::new();
        let check_interval = Duration::from_millis(50);

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
                callback(
                    ready_events,
                    command_dir,
                    command,
                    command_args,
                    event_allowed,
                    status_messages.clone(),
                );
            }
            thread::sleep(check_interval)
        }
    }

    pub fn watch(&mut self, path: &Path, mode: RecursiveMode) -> notify::Result<()> {
        self.watcher.watch(path, mode)
    }
}

fn command_exec_callback(
    events: Vec<Event>,
    command_dir: &Path,
    command: &str,
    command_args: &[&str],
    event_allowed: impl Fn(&EventKind) -> bool,
    status_messages: StatusMessages,
) {
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

        if !event_allowed(&event_kind) {
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
        let status = Command::new(command)
            .args(command_args)
            .current_dir(command_dir)
            .status()
            .expect(&status_messages.panic_message);
        if status.success() {
            println!("{}", &status_messages.success_message);
        } else {
            eprintln!("{}", &status_messages.fail_message);
        }
    }
}
