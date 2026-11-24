use std::{fs, process::Command};
fn app(dir: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_watchlist"));
    command.env("WATCHLIST_DATA_DIR", dir);
    command
}
#[test]
fn help_never_deletes_records_and_unknown_options_fail() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".watchlist");
    fs::write(&path, "project\\0pwd\\0\n").unwrap();
    let before = fs::read(&path).unwrap();
    assert!(
        app(dir.path())
            .args(["--help", "--delete", "0"])
            .output()
            .unwrap()
            .status
            .success()
    );
    assert_eq!(fs::read(&path).unwrap(), before);
    let output = app(dir.path())
        .args(["--delete", "0", "--typo"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(fs::read(&path).unwrap(), before);
    assert!(!String::from_utf8_lossy(&output.stderr).contains("panicked"));
}
#[test]
fn legacy_listing_and_delete_remain_compatible() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".watchlist");
    fs::write(&path, "project\\0pwd\\0\n").unwrap();
    let output = app(dir.path()).arg("-l").output().unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("Record Index: 0"));
    assert!(
        app(dir.path())
            .args(["-d", "0", "-l"])
            .output()
            .unwrap()
            .status
            .success()
    );
    assert!(fs::read_to_string(path).unwrap().is_empty());
}
#[test]
fn malformed_commands_and_file_paths_report_errors() {
    let dir = tempfile::tempdir().unwrap();
    let output = app(dir.path())
        .args(["-e", "sh 'unterminated"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!String::from_utf8_lossy(&output.stderr).contains("panicked"));
    let file = dir.path().join("input.rs");
    fs::write(&file, "").unwrap();
    let output = app(dir.path())
        .arg("-p")
        .arg(file)
        .args(["-e", "unused"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("must be a directory"));
}

#[test]
fn tui_requires_a_terminal_and_help_remains_noninteractive() {
    let dir = tempfile::tempdir().unwrap();
    let output = app(dir.path()).arg("--tui").output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("interactive terminal"));
    let output = app(dir.path()).args(["--tui", "--help"]).output().unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("--tui"));
    for args in [["--tui", "--delete", "0"], ["--tui", "--exec", "program"]] {
        assert!(
            !app(dir.path())
                .args(args)
                .output()
                .unwrap()
                .status
                .success()
        );
    }
}

#[test]
fn labels_display_only_for_named_records() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".watchlist");
    fs::write(&path, "project\\0pwd\\0\n").unwrap();
    let output = app(dir.path()).arg("--list").output().unwrap();
    assert!(output.status.success());
    assert!(!String::from_utf8_lossy(&output.stdout).contains("Label:"));
    let store = watchlist::saver::Store::new(&path);
    let original = store.records().unwrap().remove(0);
    let mut labelled = original.clone();
    labelled.set_label("My build");
    store.replace_record(0, &original, &labelled).unwrap();
    let output = app(dir.path()).arg("--list").output().unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("Label: My build"));
    labelled.set_label("");
    store
        .replace_record(0, &store.records().unwrap()[0], &labelled)
        .unwrap();
    let output = app(dir.path()).arg("--list").output().unwrap();
    assert!(output.status.success());
    assert!(!String::from_utf8_lossy(&output.stdout).contains("Label:"));
}
