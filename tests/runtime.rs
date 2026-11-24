use notify::RecursiveMode;
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use watchlist::{
    config::{ExecutionPolicy, WatchOptions},
    saver::Store,
    watcher::DebouncedWatcher,
};

// Spawn this integration-test executable as a portable child process fixture.
#[test]
fn child_helper() {
    let Ok(scenario) = fs::read_to_string("child-scenario") else {
        return;
    };
    let delay: u64 = scenario.trim().parse().unwrap();
    let mut log = OpenOptions::new()
        .create(true)
        .append(true)
        .open("child-log")
        .unwrap();
    writeln!(log, "start").unwrap();
    log.flush().unwrap();
    thread::sleep(Duration::from_millis(delay));
    writeln!(log, "done").unwrap();
}
#[test]
fn storage_helper() {
    let Some(path) = std::env::var_os("WATCHLIST_TEST_STORE") else {
        return;
    };
    let store = Store::new(PathBuf::from(path));
    for _ in 0..5 {
        store
            .append_line(format!("process-{}", std::process::id()))
            .unwrap();
    }
}
fn count(path: &Path, marker: &str) -> usize {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter(|s| *s == marker)
        .count()
}
fn wait_for(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !condition() {
        assert!(Instant::now() < deadline, "timed out waiting for condition");
        thread::sleep(Duration::from_millis(20));
    }
}
struct Fixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
    watched: PathBuf,
    log: PathBuf,
}
impl Fixture {
    fn new(delay: u64) -> Self {
        let dir = tempfile::tempdir().unwrap();
        // Exercise the Windows temp-directory ancestor collision on every OS.
        // The built-in **/Temp/** exclusion must only apply below the watch root.
        let root = dir.path().join("Temp").join("project");
        let watched = root.join("watched");
        fs::create_dir_all(&watched).unwrap();
        fs::write(root.join("child-scenario"), delay.to_string()).unwrap();
        fs::write(watched.join("input.rs"), "initial").unwrap();
        Self {
            log: root.join("child-log"),
            root,
            watched,
            _dir: dir,
        }
    }
    fn options(&self) -> WatchOptions {
        WatchOptions {
            workdir: Some(self.root.clone()),
            debounce_ms: 80,
            ..WatchOptions::default()
        }
    }
    fn watcher(&self, options: WatchOptions) -> DebouncedWatcher {
        DebouncedWatcher::with_options(
            self.watched.clone(),
            std::env::current_exe().unwrap().to_str().unwrap().into(),
            vec![
                "--exact".into(),
                "child_helper".into(),
                "--nocapture".into(),
            ],
            options,
        )
        .unwrap()
    }
}
#[test]
fn native_changes_survive_reads_and_directory_rename() {
    let f = Fixture::new(0);
    let mut watcher = f.watcher(f.options());
    watcher.watch(&f.watched, RecursiveMode::Recursive).unwrap();
    fs::write(f.watched.join("input.rs"), "changed").unwrap();
    fs::read(f.watched.join("input.rs")).unwrap();
    wait_for(|| count(&f.log, "done") >= 1);
    // Let any backend-specific trailing notifications settle.
    thread::sleep(Duration::from_millis(300));
    fs::create_dir(f.watched.join("subdirectory")).unwrap();
    thread::sleep(Duration::from_millis(300));
    let before = count(&f.log, "done");
    fs::rename(f.watched.join("subdirectory"), f.watched.join("renamed")).unwrap();
    wait_for(|| count(&f.log, "done") > before);
    watcher.shutdown().unwrap();
}
#[test]
fn polling_observes_changes() {
    let f = Fixture::new(0);
    let mut options = f.options();
    options.poll_ms = Some(50);
    let mut watcher = f.watcher(options);
    watcher.watch(&f.watched, RecursiveMode::Recursive).unwrap();
    thread::sleep(Duration::from_millis(150));
    fs::write(f.watched.join("input.rs"), "polling-change").unwrap();
    wait_for(|| count(&f.log, "done") >= 1);
    watcher.shutdown().unwrap();
}
#[test]
fn queue_coalesces_to_one_rerun() {
    let f = Fixture::new(600);
    let mut watcher = f.watcher(f.options());
    watcher.trigger();
    wait_for(|| count(&f.log, "start") == 1);
    for _ in 0..1000 {
        watcher.trigger();
    }
    wait_for(|| count(&f.log, "done") == 2);
    thread::sleep(Duration::from_millis(200));
    assert_eq!(count(&f.log, "start"), 2);
    watcher.shutdown().unwrap();
}
#[test]
fn ignore_drops_changes_during_execution() {
    let f = Fixture::new(300);
    let mut options = f.options();
    options.policy = ExecutionPolicy::Ignore;
    let mut watcher = f.watcher(options);
    watcher.trigger();
    wait_for(|| count(&f.log, "start") == 1);
    for _ in 0..100 {
        watcher.trigger();
    }
    wait_for(|| count(&f.log, "done") == 1);
    thread::sleep(Duration::from_millis(200));
    assert_eq!(count(&f.log, "start"), 1);
    watcher.shutdown().unwrap();
}
#[test]
fn restart_cancels_previous_execution() {
    let f = Fixture::new(500);
    let mut options = f.options();
    options.policy = ExecutionPolicy::Restart;
    let mut watcher = f.watcher(options);
    watcher.trigger();
    wait_for(|| count(&f.log, "start") == 1);
    watcher.trigger();
    wait_for(|| count(&f.log, "start") == 2);
    wait_for(|| count(&f.log, "done") == 1);
    assert_eq!(count(&f.log, "done"), 1);
    watcher.shutdown().unwrap();
}
#[test]
fn timeout_and_shutdown_cancel_running_children() {
    let f = Fixture::new(600);
    let mut options = f.options();
    options.timeout_ms = Some(120);
    let mut watcher = f.watcher(options);
    watcher.trigger();
    wait_for(|| count(&f.log, "start") == 1);
    thread::sleep(Duration::from_millis(700));
    assert_eq!(count(&f.log, "done"), 0);
    assert!(!watcher.is_finished());
    watcher.trigger();
    wait_for(|| count(&f.log, "start") == 2);
    let start = Instant::now();
    watcher.shutdown().unwrap();
    assert!(start.elapsed() < Duration::from_secs(3));
    thread::sleep(Duration::from_millis(650));
    assert_eq!(count(&f.log, "done"), 0);
}
#[test]
fn missing_executable_does_not_kill_worker() {
    let f = Fixture::new(0);
    let executable = f.root.join("recovered.exe");
    let mut watcher = DebouncedWatcher::with_options(
        f.watched.clone(),
        executable.to_str().unwrap().into(),
        vec![
            "--exact".into(),
            "child_helper".into(),
            "--nocapture".into(),
        ],
        f.options(),
    )
    .unwrap();
    watcher.trigger();
    thread::sleep(Duration::from_millis(250));
    assert!(!watcher.is_finished());
    fs::copy(std::env::current_exe().unwrap(), executable).unwrap();
    watcher.trigger();
    wait_for(|| count(&f.log, "done") == 1);
    assert!(!watcher.is_finished());
    watcher.shutdown().unwrap();
}
#[test]
fn separate_processes_cannot_lose_or_duplicate_updates() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("records");
    let mut children: Vec<_> = (0..8)
        .map(|_| {
            Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "storage_helper"])
                .env("WATCHLIST_TEST_STORE", &path)
                .stdout(Stdio::null())
                .spawn()
                .unwrap()
        })
        .collect();
    for child in &mut children {
        assert!(child.wait().unwrap().success());
    }
    assert_eq!(Store::new(path).lines().unwrap().len(), 8);
}

struct App(std::process::Child);
impl Drop for App {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
#[test]
fn legacy_exec_save_and_use_execute_the_same_arguments() {
    let f = Fixture::new(0);
    let data = f.root.join("data");
    let command = shell_words::join([
        std::env::current_exe().unwrap().to_str().unwrap(),
        "--exact",
        "child_helper",
        "--nocapture",
    ]);
    let mut app = App(Command::new(env!("CARGO_BIN_EXE_watchlist"))
        .env("WATCHLIST_DATA_DIR", &data)
        .arg("--path")
        .arg(&f.watched)
        .arg("--workdir")
        .arg(&f.root)
        .args([
            "--exec",
            &command,
            "--label",
            "Build tests",
            "--save",
            "--initial",
            "--debounce",
            "20",
        ])
        .stdout(Stdio::null())
        .spawn()
        .unwrap());
    wait_for(|| count(&f.log, "done") == 1);
    let saved = Store::new(data.join(".watchlist")).records().unwrap();
    assert_eq!(saved[0].label.as_deref(), Some("Build tests"));
    assert_eq!(saved[0].args, ["--exact", "child_helper", "--nocapture"]);
    #[cfg(unix)]
    {
        // SAFETY: this PID belongs to the child held by App.
        unsafe {
            libc::kill(app.0.id() as i32, libc::SIGTERM);
        }
        assert!(app.0.wait().unwrap().success());
    }
    #[cfg(not(unix))]
    {
        app.0.kill().unwrap();
        app.0.wait().unwrap();
    }
    let mut replay = App(Command::new(env!("CARGO_BIN_EXE_watchlist"))
        .env("WATCHLIST_DATA_DIR", &data)
        .args(["--use", "0"])
        .stdout(Stdio::null())
        .spawn()
        .unwrap());
    wait_for(|| count(&f.log, "done") == 2);
    assert!(replay.0.try_wait().unwrap().is_none());
}
