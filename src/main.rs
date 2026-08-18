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

                    let initial_split: Vec<&str> = arg_val.split(" ").collect();

                    if initial_split.is_empty() || initial_split[0].is_empty() {
                        eprintln!("No exec command provided");
                        invalid_flags = true;
                    }

                    let split_args: Vec<String> = if initial_split.first() == Some(&"sh") {
                        shell_words::split(arg_val).expect("Failed to parse shell command")
                    } else {
                        initial_split.iter().map(|s| s.to_string()).collect()
                    };

                    println!("Full Command Components: {:?}", &split_args);

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
                            arg_val.clone().parse::<usize>().unwrap(),
                        )?;
                        let parts: Vec<&str> = record.split("\\0").collect();
                        println!(
                            "\n=======Using Record Index: {}=======",
                            arg_val.parse::<usize>().unwrap()
                        );
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
                                let initial_split: Vec<&str> = parts[2].split(" ").collect();

                                command_args = if parts[1] == "sh" {
                                    shell_words::split(parts[2])
                                        .expect("Failed to parse loaded shell command arguments")
                                } else {
                                    initial_split.iter().map(|s| s.to_string()).collect()
                                };
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

        let full_command = format!("{} {}", &command, shell_words::join(&command_args));
        println!("Exec command waiting: {}", &full_command);

        if should_save_command {
            // saving
            let args_str = shell_words::join(&command_args); // quotes elements containing whitespace
            let save_line = format!("{}\\0{}\\0{}", &path, &command, &args_str);
            saver::write_new_line(SAVE_FILE_PATH, &save_line)?;

            println!(
                "\n========Saved watchlist record:========= \nWatch path: {} \nCommand: {} \nCommand Args: {}",
                &path,
                &command,
                &command_args.join(" ")
            )
        }

        let panic_message = format!(
            "Fatal error occured when running {} on {}",
            &full_command, &path
        );
        let success_message = format!("Command {} succeeded on {}", &full_command, &path);
        let fail_message = format!("Command {} failed on {}", &full_command, &path);
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
            included_events,
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

fn included_events(event_kind: &EventKind) -> bool {
    match event_kind {
        EventKind::Any => true,
        EventKind::Modify(_) => true,
        EventKind::Create(_) => true,
        EventKind::Remove(_) => true,
        _ => false,
    }
}

fn print_help() {
    println!(
        "Usage: \n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
        "-h | --help: Prints this help message",
        "-p | --path <path>: Path to watch",
        "-e | --exec <command>: Command to run on change",
        "-s | --save: Save the current --path and --exec command to the watch record list",
        "-l | --list: List all watch records with their indices",
        "-d | --delete <index>: Delete a watch record by its index",
        "-u | --use <index>: Use a watch record by its index",
        "\nExample: watchlist --path . --exec \"npm run build\" --save",
        "Explaination: Watches the current directory for changes and runs \"npm run build\" on change",
        "\nExample: watchlist --use 0",
        "Explaination: Uses the saved watch record at index 0 to watch for changes and run the associated command",
    );
}
