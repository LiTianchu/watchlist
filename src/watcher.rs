use crate::{
    config::{ExecutionPolicy, WatchOptions},
    patterns::{IGNORE_PATTERNS, WATCH_PATTERNS},
};
use notify::{Config, Event, EventKind, PollWatcher, RecommendedWatcher, RecursiveMode, Watcher};
use std::{
    io,
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus},
    sync::{Arc, Condvar, Mutex},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

type EventAllowedPredicate = fn(&EventKind) -> bool;

#[derive(Debug, Clone)]
pub struct StatusMessages {
    pub panic_message: String,
    pub success_message: String,
    pub fail_message: String,
}

/// Constant-size debounce state. Irrelevant events never reach this component.
#[derive(Debug, Default)]
struct Debounce {
    first: Option<Instant>,
    last: Option<Instant>,
}
impl Debounce {
    fn mark(&mut self, now: Instant) {
        self.first.get_or_insert(now);
        self.last = Some(now);
    }
    fn ready(&self, now: Instant, quiet: Duration, max_wait: Duration) -> bool {
        self.first
            .is_some_and(|first| now.duration_since(first) >= max_wait)
            || self
                .last
                .is_some_and(|last| now.duration_since(last) >= quiet)
    }
    fn clear(&mut self) {
        self.first = None;
        self.last = None;
    }
}

struct Filter {
    root: PathBuf,
    includes: Vec<glob::Pattern>,
    excludes: Vec<glob::Pattern>,
}
impl Filter {
    fn new(root: PathBuf, options: &WatchOptions) -> Result<Self, String> {
        let includes: Vec<_> = if options.includes.is_empty() {
            WATCH_PATTERNS.iter().map(|s| s.to_string()).collect()
        } else {
            options.includes.clone()
        };
        let mut excludes = options.excludes.clone();
        if options.default_ignores {
            excludes.extend(IGNORE_PATTERNS.iter().map(|s| s.to_string()));
        }
        let compile = |patterns: Vec<String>| {
            patterns
                .iter()
                .map(|p| glob::Pattern::new(p).map_err(|e| format!("invalid pattern {p:?}: {e}")))
                .collect::<Result<Vec<_>, _>>()
        };
        Ok(Self {
            root,
            includes: compile(includes)?,
            excludes: compile(excludes)?,
        })
    }
    fn relevant(&self, event: &Event, allowed: EventAllowedPredicate) -> bool {
        if event.need_rescan() {
            return true;
        }
        if !allowed(&event.kind) {
            return false;
        }
        let structural = matches!(
            event.kind,
            EventKind::Create(notify::event::CreateKind::Folder)
                | EventKind::Remove(notify::event::RemoveKind::Folder)
                | EventKind::Modify(notify::event::ModifyKind::Name(_))
        );
        event.paths.iter().any(|path| {
            let relative = path.strip_prefix(&self.root).unwrap_or(path);
            // Normalize separators for portable configuration patterns.
            let relative = relative.to_string_lossy().replace('\\', "/");
            let absolute = path.to_string_lossy().replace('\\', "/");
            // Relative rules describe contents of the selected root. Matching
            // their absolute ancestors would exclude every project in Windows'
            // user Temp directory (and projects under build/target elsewhere).
            let target = |p: &glob::Pattern| {
                if Path::new(p.as_str()).is_absolute() {
                    absolute.as_str()
                } else {
                    relative.as_str()
                }
            };
            let matches = |p: &glob::Pattern| p.matches(target(p));
            let excluded = self
                .excludes
                .iter()
                .any(|p| matches(p) || p.matches(&format!("{}/", target(p))));
            !excluded && (structural || path.is_dir() || self.includes.iter().any(matches))
        })
    }
}

pub fn included_events(kind: &EventKind) -> bool {
    matches!(
        kind,
        EventKind::Any
            | EventKind::Create(_)
            | EventKind::Modify(_)
            | EventKind::Remove(_)
            | EventKind::Access(notify::event::AccessKind::Close(
                notify::event::AccessMode::Write
            ))
    )
}

#[derive(Default)]
struct State {
    stop: bool,
    running: bool,
    pending: Debounce,
    // Retain only the latest diagnostic, even during error storms.
    error: Option<String>,
    error_count: u64,
}
struct Shared {
    state: Mutex<State>,
    wake: Condvar,
    policy: ExecutionPolicy,
}
impl Shared {
    fn event(
        &self,
        result: notify::Result<Event>,
        filter: &Filter,
        allowed: EventAllowedPredicate,
    ) {
        match result {
            Ok(event) if filter.relevant(&event, allowed) => self.mark(),
            Ok(_) => {}
            Err(error) => {
                let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
                state.error = Some(error.to_string());
                state.error_count = state.error_count.saturating_add(1);
                // Conservatively rerun after lost events; report degraded monitoring.
                state.pending.mark(Instant::now());
                self.wake.notify_one();
            }
        }
    }
    fn mark(&self) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if !(state.stop || state.running && self.policy == ExecutionPolicy::Ignore) {
            state.pending.mark(Instant::now());
            self.wake.notify_one();
        }
    }
}

pub struct DebouncedWatcher {
    watcher: Option<Box<dyn Watcher + Send>>,
    shared: Arc<Shared>,
    worker: Option<JoinHandle<io::Result<()>>>,
}
impl DebouncedWatcher {
    /// Compatibility constructor; the worker now shuts down when this value is dropped.
    pub fn new(
        watch_path: impl Into<String> + Send + 'static,
        debounce_duration: Duration,
        command: String,
        command_args: Vec<String>,
        event_allowed: EventAllowedPredicate,
        status_messages: StatusMessages,
    ) -> notify::Result<Self> {
        let options = WatchOptions {
            debounce_ms: debounce_duration
                .as_millis()
                .try_into()
                .map_err(|_| notify::Error::generic("debounce duration too large"))?,
            ..WatchOptions::default()
        };
        Self::build(
            watch_path.into().into(),
            command,
            command_args,
            options,
            event_allowed,
            Some(status_messages),
        )
    }
    pub fn with_options(
        path: PathBuf,
        command: String,
        args: Vec<String>,
        options: WatchOptions,
    ) -> notify::Result<Self> {
        Self::build(path, command, args, options, included_events, None)
    }
    fn build(
        path: PathBuf,
        command: String,
        args: Vec<String>,
        options: WatchOptions,
        allowed: EventAllowedPredicate,
        messages: Option<StatusMessages>,
    ) -> notify::Result<Self> {
        options.validate().map_err(|e| notify::Error::generic(&e))?;
        let filter = Filter::new(path.clone(), &options).map_err(|e| notify::Error::generic(&e))?;
        let shared = Arc::new(Shared {
            state: Mutex::new(State::default()),
            wake: Condvar::new(),
            policy: options.policy,
        });
        let sender = shared.clone();
        let handler = move |result| sender.event(result, &filter, allowed);
        let watcher: Box<dyn Watcher + Send> = if let Some(ms) = options.poll_ms {
            Box::new(PollWatcher::new(
                handler,
                Config::default()
                    .with_poll_interval(Duration::from_millis(ms))
                    .with_compare_contents(true),
            )?)
        } else {
            Box::new(RecommendedWatcher::new(handler, Config::default())?)
        };
        let worker_state = shared.clone();
        let worker = thread::Builder::new()
            .name("watchlist-worker".into())
            .spawn(move || worker_loop(worker_state, path, command, args, options, messages))
            .map_err(notify::Error::io)?;
        Ok(Self {
            watcher: Some(watcher),
            shared,
            worker: Some(worker),
        })
    }
    pub fn watch(&mut self, path: &Path, mode: RecursiveMode) -> notify::Result<()> {
        self.watcher
            .as_mut()
            .ok_or_else(|| notify::Error::generic("watcher has shut down"))?
            .watch(path, mode)
    }
    pub fn trigger(&self) {
        self.shared.mark();
    }
    pub fn is_finished(&self) -> bool {
        self.worker.as_ref().is_none_or(|w| w.is_finished())
    }
    pub fn shutdown(&mut self) -> io::Result<()> {
        {
            let mut state = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
            state.stop = true;
            self.shared.wake.notify_one();
        }
        // No callback needs the worker to drain a channel before it can exit.
        self.watcher.take();
        if let Some(worker) = self.worker.take() {
            worker
                .join()
                .map_err(|_| io::Error::other("watcher worker panicked"))??;
        }
        Ok(())
    }
}
impl Drop for DebouncedWatcher {
    fn drop(&mut self) {
        if let Err(e) = self.shutdown() {
            eprintln!("watchlist: shutdown: {e}");
        }
    }
}

struct Running {
    child: Child,
    started: Instant,
    reaped: bool,
}
impl Running {
    fn spawn(command: &str, args: &[String], dir: &Path) -> io::Result<Self> {
        let mut cmd = Command::new(command);
        cmd.args(args).current_dir(dir);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        Ok(Self {
            child: cmd.spawn()?,
            started: Instant::now(),
            reaped: false,
        })
    }
    fn poll(&mut self) -> io::Result<Option<ExitStatus>> {
        let status = self.child.try_wait()?;
        if status.is_some() {
            self.reaped = true;
        }
        Ok(status)
    }
    fn cancel(&mut self) -> io::Result<()> {
        self.cancel_with_tree_signal(signal_process_tree)
    }

    fn cancel_with_tree_signal(
        &mut self,
        signal_tree: impl FnOnce(&mut Child) -> io::Result<()>,
    ) -> io::Result<()> {
        // Shutdown can arrive after the child exits but before the worker polls it.
        if self.reaped || self.poll()?.is_some() {
            return Ok(());
        }
        if let Err(error) = signal_tree(&mut self.child) {
            // The child can also exit between the check above and signalling.
            // Darwin may return EPERM for a group containing only zombies.
            if self.poll()?.is_some() {
                return Ok(());
            }
            return Err(error);
        }
        // Also cover a process that escaped its original group. Never wait
        // indefinitely after a failed kill of a child that is still alive.
        if let Err(error) = self.child.kill() {
            if self.poll()?.is_some() {
                return Ok(());
            }
            return Err(error);
        }
        self.child.wait()?;
        self.reaped = true;
        Ok(())
    }
}

fn signal_process_tree(child: &mut Child) -> io::Result<()> {
    #[cfg(unix)]
    {
        // The child leads a dedicated process group; cancel descendants too.
        let pid = i32::try_from(child.id()).map_err(io::Error::other)?;
        // SAFETY: kill targets only the process group created in Running::spawn().
        if unsafe { libc::kill(-pid, libc::SIGKILL) } != 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(libc::ESRCH) {
                return Err(error);
            }
        }
    }
    #[cfg(windows)]
    {
        // taskkill is supplied by Windows and terminates the descendant tree.
        let _ = Command::new("taskkill")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            // A child may exit during taskkill's startup. Direct-child cleanup
            // below checks errors without printing benign "process not found".
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
    Ok(())
}
impl Drop for Running {
    fn drop(&mut self) {
        if let Err(e) = self.cancel() {
            eprintln!("watchlist: child cleanup: {e}");
        }
    }
}

fn worker_loop(
    shared: Arc<Shared>,
    path: PathBuf,
    command: String,
    args: Vec<String>,
    options: WatchOptions,
    messages: Option<StatusMessages>,
) -> io::Result<()> {
    let quiet = Duration::from_millis(options.debounce_ms);
    let max_wait = Duration::from_millis(options.max_wait_ms);
    let mut running: Option<Running> = None;
    let dir = options.workdir.as_deref().unwrap_or(&path);
    loop {
        let mut state = shared.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.stop {
            break;
        }
        if let Some(error) = state.error.take() {
            eprintln!(
                "watchlist: watcher error ({} since last report): {error}; scheduling recovery run; monitoring may be degraded",
                state.error_count
            );
            state.error_count = 0;
        }
        drop(state);
        if let Some(child) = &mut running {
            if let Some(status) = child.poll()? {
                if let Some(messages) = &messages {
                    if status.success() {
                        println!("{}", messages.success_message);
                    } else {
                        eprintln!("{} ({status})", messages.fail_message);
                    }
                } else {
                    println!(
                        "Command finished: {status}; elapsed: {} ms",
                        child.started.elapsed().as_millis()
                    );
                }
                running = None;
            } else if options
                .timeout_ms
                .is_some_and(|ms| child.started.elapsed() >= Duration::from_millis(ms))
            {
                eprintln!("watchlist: command timed out");
                child.cancel()?;
                running = None;
            }
        }
        let mut state = shared.state.lock().unwrap_or_else(|e| e.into_inner());
        state.running = running.is_some();
        if state.stop {
            break;
        }
        let ready = state.pending.ready(Instant::now(), quiet, max_wait);
        let start = ready && (running.is_none() || options.policy == ExecutionPolicy::Restart);
        if start {
            state.pending.clear();
            state.running = true;
            drop(state);
            if let Some(mut child) = running.take() {
                child.cancel()?;
            }
            match Running::spawn(&command, &args, dir) {
                Ok(child) => {
                    println!("Command started (pid {})", child.child.id());
                    running = Some(child);
                }
                Err(error) => {
                    eprintln!(
                        "watchlist: could not start {command:?}: {error}; continuing to watch"
                    );
                    shared
                        .state
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .running = false;
                }
            }
            continue;
        }
        // Sleep until a deadline/event when idle; poll only a running child.
        let wait = if running.is_some() {
            Some(Duration::from_millis(20))
        } else {
            state
                .pending
                .first
                .zip(state.pending.last)
                .map(|(first, last)| {
                    (first + max_wait)
                        .min(last + quiet)
                        .saturating_duration_since(Instant::now())
                })
        };
        if let Some(wait) = wait {
            drop(
                shared
                    .wake
                    .wait_timeout(state, wait)
                    .unwrap_or_else(|e| e.into_inner()),
            );
        } else {
            drop(shared.wake.wait(state).unwrap_or_else(|e| e.into_inner()));
        }
    }
    if let Some(mut child) = running {
        child.cancel()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{AccessKind, AccessMode, CreateKind, ModifyKind, RenameMode};
    fn shared(policy: ExecutionPolicy) -> Shared {
        Shared {
            state: Mutex::new(State::default()),
            wake: Condvar::new(),
            policy,
        }
    }
    fn event(kind: EventKind, path: &str) -> Event {
        Event::new(kind).add_path(PathBuf::from(path))
    }
    #[test]
    fn debounce_quiet_and_max_wait_without_sleeping() {
        let now = Instant::now();
        let mut d = Debounce::default();
        let quiet = Duration::from_millis(500);
        let max = Duration::from_secs(2);
        assert!(!d.ready(now, quiet, max));
        d.mark(now);
        assert!(!d.ready(now + Duration::from_millis(499), quiet, max));
        d.mark(now + Duration::from_millis(400));
        assert!(!d.ready(now + Duration::from_millis(700), quiet, max));
        assert!(d.ready(now + Duration::from_millis(900), quiet, max));
        for ms in [800, 1200, 1600, 1999] {
            d.mark(now + Duration::from_millis(ms));
        }
        assert!(d.ready(now + max, quiet, max));
        d.clear();
        assert!(!d.ready(now + max, quiet, max));
    }
    #[test]
    fn reads_cannot_overwrite_or_delay_changes() {
        let filter = Filter::new(PathBuf::from("project"), &WatchOptions::default()).unwrap();
        let shared = shared(ExecutionPolicy::Queue);
        shared.event(
            Ok(event(EventKind::Modify(ModifyKind::Any), "project/file.rs")),
            &filter,
            included_events,
        );
        let before = shared.state.lock().unwrap().pending.last;
        for kind in [
            AccessKind::Open(AccessMode::Read),
            AccessKind::Close(AccessMode::Read),
        ] {
            shared.event(
                Ok(event(EventKind::Access(kind), "project/file.rs")),
                &filter,
                included_events,
            );
        }
        assert_eq!(shared.state.lock().unwrap().pending.last, before);
    }
    #[test]
    fn directory_moves_and_deletions_are_relevant() {
        let filter = Filter::new(PathBuf::from("project"), &WatchOptions::default()).unwrap();
        for kind in [
            EventKind::Modify(ModifyKind::Name(RenameMode::Both)),
            EventKind::Remove(notify::event::RemoveKind::Folder),
            EventKind::Create(CreateKind::Folder),
        ] {
            assert!(filter.relevant(&event(kind, "project/subdirectory"), included_events));
            assert!(!filter.relevant(&event(kind, "project/target"), included_events));
        }
    }
    #[test]
    fn custom_filters_and_legacy_defaults() {
        let options = WatchOptions::default();
        let root = PathBuf::from("project");
        let filter = Filter::new(root.clone(), &options).unwrap();
        let change = |path| event(EventKind::Modify(ModifyKind::Any), path);
        assert!(filter.relevant(&change("project/src/a.rs"), included_events));
        for path in [
            "project/README.md",
            "project/target/a.rs",
            "project/assets/a.png",
            "project/Cargo.lock",
        ] {
            assert!(!filter.relevant(&change(path), included_events), "{path}");
        }
        let options = WatchOptions {
            includes: vec!["**/*.md".into()],
            excludes: vec!["**/private/**".into()],
            default_ignores: false,
            ..options
        };
        let filter = Filter::new(root, &options).unwrap();
        assert!(filter.relevant(&change("project/README.md"), included_events));
        assert!(!filter.relevant(&change("project/private/a.md"), included_events));
        assert!(!filter.relevant(&change("project/a.rs"), included_events));
    }
    #[test]
    fn ignores_apply_inside_the_watch_root_not_to_its_ancestors() {
        for root in [
            "project/Temp/workspace",
            "project/target/workspace",
            "C:/Users/runneradmin/AppData/Local/Temp/project",
        ] {
            let root = PathBuf::from(root);
            let filter = Filter::new(root.clone(), &WatchOptions::default()).unwrap();
            let change = |name: &str| {
                Event::new(EventKind::Modify(ModifyKind::Any)).add_path(root.join(name))
            };
            assert!(
                filter.relevant(&change("input.rs"), included_events),
                "root: {}",
                root.display()
            );
            for ignored in ["Temp/input.rs", "target/input.rs", "README.md"] {
                assert!(!filter.relevant(&change(ignored), included_events));
            }
        }
    }
    #[test]
    fn custom_rules_are_root_relative_unless_explicitly_absolute() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Temp").join("project");
        let mut options = WatchOptions {
            excludes: vec!["**/Temp/**".into()],
            default_ignores: false,
            ..WatchOptions::default()
        };
        let change =
            |path: &str| Event::new(EventKind::Modify(ModifyKind::Any)).add_path(root.join(path));
        let filter = Filter::new(root.clone(), &options).unwrap();
        assert!(filter.relevant(&change("src/input.rs"), included_events));
        assert!(!filter.relevant(&change("Temp/input.rs"), included_events));

        options.includes = vec![root.join("src/*.rs").to_string_lossy().replace('\\', "/")];
        options.excludes = vec![
            root.join("src/blocked.rs")
                .to_string_lossy()
                .replace('\\', "/"),
        ];
        let filter = Filter::new(root.clone(), &options).unwrap();
        assert!(filter.relevant(&change("src/input.rs"), included_events));
        assert!(!filter.relevant(&change("src/blocked.rs"), included_events));
        assert!(!filter.relevant(&change("other/input.rs"), included_events));
    }
    #[test]
    fn errors_and_pathless_rescan_schedule_recovery() {
        let filter = Filter::new(PathBuf::from("project"), &WatchOptions::default()).unwrap();
        let shared = shared(ExecutionPolicy::Queue);
        shared.event(
            Err(notify::Error::generic("lost events")),
            &filter,
            included_events,
        );
        let mut state = shared.state.lock().unwrap();
        assert!(state.error.is_some());
        assert!(state.pending.first.is_some());
        state.pending.clear();
        drop(state);
        shared.event(
            Ok(Event::new(EventKind::Other).set_flag(notify::event::Flag::Rescan)),
            &filter,
            included_events,
        );
        assert!(shared.state.lock().unwrap().pending.first.is_some());
    }
    #[test]
    fn scheduling_is_constant_size_and_ignore_is_explicit() {
        for policy in [
            ExecutionPolicy::Queue,
            ExecutionPolicy::Restart,
            ExecutionPolicy::Ignore,
        ] {
            let shared = shared(policy);
            shared.state.lock().unwrap().running = true;
            for _ in 0..10000 {
                shared.mark();
            }
            assert_eq!(
                shared.state.lock().unwrap().pending.first.is_some(),
                policy != ExecutionPolicy::Ignore
            );
        }
    }
    #[test]
    fn drop_joins_and_shutdown_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let mut watcher = DebouncedWatcher::with_options(
            dir.path().into(),
            "unused".into(),
            vec![],
            WatchOptions::default(),
        )
        .unwrap();
        let shared = watcher.shared.clone();
        watcher.shutdown().unwrap();
        assert!(watcher.worker.is_none());
        assert!(shared.state.lock().unwrap().stop);
        watcher.shutdown().unwrap();
        assert!(watcher.watch(dir.path(), RecursiveMode::Recursive).is_err());
    }
    #[test]
    fn worker_panics_are_reported() {
        let dir = tempfile::tempdir().unwrap();
        let mut watcher = DebouncedWatcher::with_options(
            dir.path().into(),
            "unused".into(),
            vec![],
            WatchOptions::default(),
        )
        .unwrap();
        watcher.shutdown().unwrap();
        watcher.worker = Some(thread::spawn(|| panic!("injected failure")));
        assert!(
            watcher
                .shutdown()
                .unwrap_err()
                .to_string()
                .contains("panicked")
        );
    }

    #[cfg(unix)]
    fn shell_child(script: &str) -> Running {
        Running::spawn("sh", &["-c".into(), script.into()], Path::new(".")).unwrap()
    }

    #[cfg(unix)]
    #[test]
    fn cancellation_reaps_zombies_before_signalling() {
        for _ in 0..16 {
            let mut running = shell_child("exit 0");
            let mut info = std::mem::MaybeUninit::<libc::siginfo_t>::uninit();
            // SAFETY: info is writable and the PID belongs to this child.
            // WNOWAIT waits for exit without reaping, deterministically retaining
            // the zombie state that triggers Darwin's group-signalling EPERM.
            assert_eq!(
                unsafe {
                    libc::waitid(
                        libc::P_PID,
                        running.child.id() as libc::id_t,
                        info.as_mut_ptr(),
                        libc::WEXITED | libc::WNOWAIT,
                    )
                },
                0
            );
            assert!(!running.reaped);
            running
                .cancel_with_tree_signal(|_| panic!("exited child must not be signalled"))
                .unwrap();
            assert!(running.reaped);
            assert!(running.child.try_wait().unwrap().unwrap().success());
            running.cancel().unwrap();
        }
    }

    #[cfg(unix)]
    #[test]
    fn cancellation_rechecks_exit_after_group_signal_error() {
        let mut running = shell_child("exec sleep 30");
        running
            .cancel_with_tree_signal(|child| {
                // Exit after cancellation's initial poll, then simulate Darwin's
                // EPERM. Child::wait caches status for the subsequent try_wait.
                child.kill()?;
                child.wait()?;
                Err(io::Error::from_raw_os_error(libc::EPERM))
            })
            .unwrap();
        assert!(running.reaped);
        running.cancel().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn cancellation_preserves_errors_for_live_children() {
        let mut running = shell_child("exec sleep 30");
        let error = running
            .cancel_with_tree_signal(|_| Err(io::Error::from_raw_os_error(libc::EPERM)))
            .unwrap_err();
        assert_eq!(error.raw_os_error(), Some(libc::EPERM));
        assert!(!running.reaped);
        assert!(running.poll().unwrap().is_none());
        // Genuine cancellation still terminates and reaps a live child.
        running.cancel().unwrap();
        assert!(running.reaped);
        assert!(!running.child.try_wait().unwrap().unwrap().success());
    }
}
