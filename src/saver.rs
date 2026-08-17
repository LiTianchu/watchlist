use std::{
    fs::OpenOptions,
    io::{self, BufRead, Error, Write},
};

pub fn read_save_lines(save_path: impl Into<String>) -> Result<Vec<String>, Error> {
    let save_path = save_path.into();

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

pub fn write_new_line(save_path: impl Into<String>, line: impl Into<String>) -> Result<(), Error> {
    let save_path = save_path.into();
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

pub fn remove_line_by_index(save_path: impl Into<String>, index: usize) -> Result<(), Error> {
    println!("Removing line at index {}", index);
    let save_path = save_path.into();
    let curr_lines = read_save_lines(save_path.clone())?;
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
