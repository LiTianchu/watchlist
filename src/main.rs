use notify::{EventKind, RecursiveMode};
use std::env;
use std::path::{Path, PathBuf};
use std::sync::mpsc::channel;
use std::time::Duration;
use watchlist::{
    saver,
    watcher::{DebouncedWatcher, StatusMessages},
};

const SAVE_FILE_PATH: &str = ".watchlist";

fn main() -> notify::Result<()> {
    let user_args: Vec<String> = env::args().collect();

    let mut path = env::current_dir();
    let mut command: String = "".to_string();
    let mut command_args: Vec<String> = Vec::new();
    let mut invalid_flags = false;
    let mut should_save_command = false;
    let mut showing_usage = false;
    let mut listing = false;
    let mut removing = false;

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
            "--use" | "-u" => {
                if i + 1 < user_args.len() {
                    let arg_val = &user_args[i + 1];
                    // check if it is a number
                    if arg_val.parse::<usize>().is_ok() {
                        let record = saver::read_line_by_index(
                            SAVE_FILE_PATH,
                            arg_val.parse::<usize>().unwrap(),
                        )?;
                        let parts: Vec<&str> = record.split("\\0").collect();
                        println!("\n=======Using Record Index: {}=======", i);
                        for (i, part) in parts.iter().enumerate() {
                            let mut label = "Unknown Record";
                            if i == 0 {
                                label = "Watch Path";
                                path = Ok(PathBuf::from(parts[0]));
                            } else if i == 1 {
                                label = "Exec Command";
                                command = parts[1].to_string();
                            } else if i == 2 {
                                label = "Command Argumnets";
                                command_args = parts[2]
                                    .split(" ")
                                    .map(|s| s.to_string())
                                    .collect::<Vec<_>>();
                            }

                            println!("{}: {}", label, part);
                        }
                    } else {
                        eprintln!("Record index argument must be a number");
                        invalid_flags = true;
                    }
                } else {
                    eprintln!("No record index argument provided");
                    invalid_flags = true;
                }
            }
            "--save" | "-s" => {
                should_save_command = true;
            }
            "--delete" | "-d" => {
                if i + 1 < user_args.len() && user_args[i + 1] != "" {
                    let arg_val = &user_args[i + 1];
                    // check if it is a number
                    if arg_val.parse::<usize>().is_ok() {
                        saver::remove_line_by_index(
                            SAVE_FILE_PATH,
                            arg_val.parse::<usize>().unwrap(),
                        )?;
                        removing = true;
                    } else {
                        eprintln!("Remove index argument must be a number");
                        invalid_flags = true;
                    }
                } else {
                    eprintln!("No remove index argument provided");
                    invalid_flags = true;
                }
            }
            "--list" | "-l" => {
                saver::read_save_lines(SAVE_FILE_PATH)
                    .expect("Read save lines failed!")
                    .iter()
                    .enumerate()
                    .for_each(|(i, s)| {
                        let parts: Vec<&str> = s.split("\\0").collect();
                        println!("\n=======Record Index: {}=======", i);
                        for (i, part) in parts.iter().enumerate() {
                            let mut label = "Unknown Record";
                            if i == 0 {
                                label = "Watch Path"
                            } else if i == 1 {
                                label = "Exec Command"
                            } else if i == 2 {
                                label = "Command Argumnets"
                            }

                            println!("{}: {}", label, part);
                        }
                    });
                listing = true;
            }
            "--help" | "-h" => {
                showing_usage = true;
            }
            _ => {}
        }
    }

    if invalid_flags {
        panic!("Invalid flags provided");
    }

    if (command == "" && !listing && !removing) || showing_usage {
        print_help();
        Ok(())
    } else if listing || removing {
        Ok(())
    } else {
        let path = path?.to_string_lossy().to_string();

        println!(
            "Exec command waiting: {} {}",
            &command,
            &command_args.join(" ")
        );

        if should_save_command {
            let save_line = format!("{}\\0{}\\0{}", &path, &command, &command_args.join(" "));
            saver::write_new_line(SAVE_FILE_PATH, &save_line)?;

            println!(
                "\n========Saved watchlist record:========= \nWatch path: {} \nCommand: {} \nCommand Args: {}",
                &path,
                &command,
                &command_args.join(" ")
            )
        }

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

        println!("\nWatching {} for changes...", path);

        // block the main thread to prevent the program from shutting down
        let (_tx, rx) = channel::<()>();
        rx.recv().ok();
        Ok(())
    }
}

fn should_rebuild(event_kind: &EventKind) -> bool {
    match event_kind {
        EventKind::Modify(_) => true,
        EventKind::Create(_) => true,
        EventKind::Remove(_) => true,
        _ => false,
    }
}

fn print_help() {
    println!("Usage: ");
}
