use std::{
    fs::OpenOptions,
    io::{self, BufRead, Error, Write, ErrorKind},
    path::PathBuf,
};

const APP_DIR_NAME: &str = "watchlist";

pub fn resolve_save_path(save_file_name: impl Into<String>) -> Result<PathBuf, Error> {
    let mut dir = dirs::data_dir()
        .ok_or_else(|| Error::new(ErrorKind::NotFound, "Could not determine user data directory"))?;

    dir.push(APP_DIR_NAME);

    // create the app's subdirectory if it doesn't exist yet
    std::fs::create_dir_all(&dir)?;

    dir.push(save_file_name.into());
    Ok(dir)
}

pub fn read_save_lines(save_file_name: impl Into<String>) -> Result<Vec<String>, Error> {
    let save_file_name = save_file_name.into();
    let save_path = resolve_save_path(save_file_name.clone())?;

    let file = OpenOptions::new()
        .read(true)
        .create(true)
        .write(true)
        .open(save_path.clone())?;

    let reader = io::BufReader::new(file);
    let mut lines = Vec::new();

    for l in reader.lines() {
        let line = l?;
        lines.push(line);
    }

    Ok(lines)
}

pub fn write_new_line(save_file_name: impl Into<String>, line: impl Into<String>) -> Result<(), Error> {
    let save_file_name = save_file_name.into();
    let save_path = resolve_save_path(save_file_name.clone())?;
    let saving_line = line.into();

    let mut file = OpenOptions::new()
        .read(true)
        .create(true)
        .append(true)
        .open(save_path.clone())?;

    let reader = io::BufReader::new(&file);

    // if line already exists, do nothing
    for l in reader.lines() {
        let line = l?;
        if line == saving_line {
            return Ok(());
        }
    }

    // perform append
    writeln!(file, "{}", saving_line)?;
    Ok(())
}

pub fn remove_line_by_index(save_file_name: impl Into<String>, index: usize) -> Result<(), Error> {
    println!("Removing line at index {}", index);
    let save_file_name = save_file_name.into();
    let save_path = resolve_save_path(save_file_name.clone())?;
    let curr_lines = read_save_lines(save_file_name.clone())?;
    let prev_line_count = curr_lines.len();
    let remaining_lines = curr_lines.iter().enumerate().filter(|(i, _)| *i != index);
    let remaining_line_count = remaining_lines.clone().collect::<Vec<_>>().len();

    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(save_path)?;

    for (_, line) in remaining_lines {
        writeln!(file, "{}", line)?;
    }

    if prev_line_count - remaining_line_count > 0 {
        println!(
            "Removed {} line at index {}",
            prev_line_count - remaining_line_count,
            index
        );
    } else {
        println!(
            "No line removed, index {} not found.\nUse -l to list all the records.",
            index
        );
    }

    Ok(())
}

pub fn read_line_by_index(save_file_name: impl Into<String>, index: usize) -> Result<String, Error> {
    let save_file_name = save_file_name.into();
    let save_path = resolve_save_path(save_file_name.clone())?;

    let file = OpenOptions::new()
        .read(true)
        .create(true)
        .write(true)
        .open(save_path.clone())?;

    let reader = io::BufReader::new(file);

    for (i, line) in reader.lines().enumerate() {
        if i == index {
            return Ok(line?);
        }
    }

    Err(Error::new(io::ErrorKind::NotFound, "Line not found"))
}
