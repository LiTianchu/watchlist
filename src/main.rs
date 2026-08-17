use file_watch::watcher::{DebouncedWatcher, StatusMessages};
use notify::{EventKind, RecursiveMode};
use std::env;
use std::path::Path;
use std::sync::mpsc::channel;
use std::time::Duration;

fn main() -> notify::Result<()> {
    let args: Vec<String> = env::args().collect();
    let path: String = args
        .iter()
        .position(|arg| arg == "--path")
        .map(|p| args.get(p + 1))
        .expect("No --path argument provided")
        .expect("Path argument is not valid")
        .clone();

    let toml_path = [path.clone(), "/Cargo.toml".to_string()].concat();
    let target_path = [path.clone(), "/target".to_string()].concat();
    let command_args = vec![
        String::from("build"),
        String::from("--manifest-path"),
        toml_path,
        String::from("--target-dir"),
        target_path,
    ];

    let panic_message = format!("failed to run cargo build on {:?}", path);
    let success_message = format!("Build cargo project succeeded: {:?}", path);
    let fail_message = format!("Build failed: {:?}", path);
    let status_messages = StatusMessages {
        panic_message,
        success_message,
        fail_message,
    };

    let mut debouncer = DebouncedWatcher::new(
        path.clone(),
        Duration::from_millis(500),
        "cargo".to_owned(),
        command_args,
        should_rebuild,
        status_messages.clone(),
    )
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
