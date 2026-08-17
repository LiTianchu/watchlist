use notify::{EventKind, RecursiveMode};
use std::env;
use std::path::Path;
use std::sync::mpsc::channel;
use std::time::Duration;
use watchlist::watcher::{DebouncedWatcher, StatusMessages};

fn main() -> notify::Result<()> {
    let user_args: Vec<String> = env::args().collect();

    let mut path = env::current_dir();
    let mut command: String = "".to_string();
    let mut command_args: Vec<String> = Vec::new();
    let mut invalid_flags = false;

    for i in 0..user_args.len() {
        let arg = &user_args[i];

        match arg.as_ref() {
            "--path" | "-p" => {
                if i + 1 < user_args.len() {
                    let arg_val = &user_args[i + 1];
                    match Path::new(arg_val).canonicalize() {
                        Ok(resolved_path) => path = Ok(resolved_path),
                        Err(e) => {
                            eprintln!("Failed to resolve path: {}", e);
                            invalid_flags = true;
                        }
                    }
                } else {
                    eprintln!("No path argument provided");
                    invalid_flags = true;
                }
            }
            "--exec" | "-e" => {
                if i + 1 < user_args.len() && user_args[i + 1] != "" {
                    let arg_val = &user_args[i + 1];
                    let split_args = arg_val.split(" ").collect::<Vec<&str>>();
                    println!("Command Args: {:?}", &split_args);

                    let _ = split_args
                        .iter()
                        .enumerate()
                        .map(|(i, c)| {
                            if i == 0 {
                                command = c.to_string();
                            } else {
                                command_args.push(c.to_string());
                            }
                        })
                        .collect::<Vec<_>>();

                    if command == "" {
                        eprintln!("No exec command provided");
                        invalid_flags = true;
                    }
                } else {
                    eprintln!("No exec command argument provided");
                    invalid_flags = true;
                }
            }
            _ => {}
        }
    }

    if invalid_flags {
        panic!("Invalid flags provided");
    }

    let path = path?.to_string_lossy().to_string();

    println!(
        "Exec command waiting: {} {}",
        &command,
        &command_args.join(" ")
    );

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
        command,
        command_args,
        should_rebuild,
        status_messages.clone(),
    )
    .expect("Debouncer failed to initialize.");

    debouncer.watch(Path::new(&path), RecursiveMode::Recursive)?;

    println!("Watching {} for changes...", path);

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
