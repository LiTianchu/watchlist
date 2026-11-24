use notify::RecursiveMode;
use std::{
    env,
    process::ExitCode,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};
use watchlist::{
    cli::{Cli, HELP},
    config::WatchOptions,
    saver::{self, Record, Store},
    watcher::DebouncedWatcher,
};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("watchlist: {e}");
            ExitCode::FAILURE
        }
    }
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse(env::args_os().skip(1))?;
    if cli.help
        || (!cli.tui
            && !cli.list
            && cli.delete.is_none()
            && cli.command.is_none()
            && cli.use_index.is_none())
    {
        print!("{HELP}");
        return Ok(());
    }
    let store = Store::new(saver::resolve_save_path(".watchlist")?);
    if cli.tui {
        return watchlist::tui::run(store, cli.use_index.unwrap_or(0)).map_err(Into::into);
    }
    if let Some(index) = cli.delete {
        if store.remove(index)? {
            println!("Removed record at index {index}");
        } else {
            println!("No record at index {index}");
        }
    }
    if cli.list {
        for (index, record) in store.records()?.iter().enumerate() {
            let label = record
                .label
                .as_ref()
                .map(|name| format!("Label: {name}\n"))
                .unwrap_or_default();
            println!(
                "\n=======Record Index: {index}=======\n{label}Watch Path: {}\nExec Command: {}\nCommand Arguments: {:?}\nOptions: {:?}",
                record.path.display(),
                record.command,
                record.args,
                record.options
            );
        }
    }
    if cli.list || cli.delete.is_some() {
        return Ok(());
    }
    let mut record = if let Some(index) = cli.use_index {
        store
            .records()?
            .get(index)
            .cloned()
            .ok_or("saved record index not found")?
    } else {
        Record {
            version: 1,
            label: None,
            path: env::current_dir()?,
            command: String::new(),
            args: vec![],
            options: WatchOptions::default(),
        }
    };
    if let Some(path) = &cli.path {
        record.path = path.clone();
    }
    if let Some(command) = &cli.command {
        record.command = command[0].clone();
        record.args = command[1..].to_vec();
    }
    if let Some(label) = &cli.label {
        record.set_label(label);
    }
    cli.apply_options(&mut record.options)?;
    record.path = record.path.canonicalize()?;
    if !record.path.is_dir() {
        return Err("--path must be a directory; use --include to select files".into());
    }
    if let Some(workdir) = &mut record.options.workdir {
        *workdir = workdir.canonicalize()?;
        if !workdir.is_dir() {
            return Err("--workdir must be a directory".into());
        }
    }
    let stop = Arc::new(AtomicBool::new(false));
    let signal_stop = stop.clone();
    ctrlc::set_handler(move || {
        signal_stop.store(true, Ordering::SeqCst);
    })?;
    let mut watcher = DebouncedWatcher::with_options(
        record.path.clone(),
        record.command.clone(),
        record.args.clone(),
        record.options.clone(),
    )?;
    watcher.watch(&record.path, RecursiveMode::Recursive)?;
    if cli.save {
        store.save(&record)?;
        println!("Saved watch configuration");
    }
    println!("Watching {} for changes...", record.path.display());
    if let Some(label) = &record.label {
        println!("Label: {label}");
    }
    println!(
        "Working directory: {}; debounce: {} ms; backend: {}; policy: {:?}",
        record
            .options
            .workdir
            .as_ref()
            .unwrap_or(&record.path)
            .display(),
        record.options.debounce_ms,
        if record.options.poll_ms.is_some() {
            "poll"
        } else {
            "native"
        },
        record.options.policy
    );
    if record.options.initial {
        watcher.trigger();
    }
    while !stop.load(Ordering::SeqCst) {
        if watcher.is_finished() {
            watcher.shutdown()?;
            return Err("watcher worker stopped unexpectedly".into());
        }
        thread::sleep(Duration::from_millis(50));
    }
    watcher.shutdown()?;
    Ok(())
}
