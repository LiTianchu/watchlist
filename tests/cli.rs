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

#[test]
fn watch_options_alone_explain_the_missing_command_instead_of_printing_help() {
    let dir = tempfile::tempdir().unwrap();
    for args in [
        vec!["--max-wait", "4000"],
        vec!["--save"],
        vec!["--path", "."],
        vec!["--label", "Build"],
        vec!["--initial"],
        vec!["--include", "**/*.rs"],
    ] {
        let output = app(dir.path()).args(&args).output().unwrap();
        assert!(!output.status.success(), "{args:?}");
        assert!(output.stdout.is_empty(), "help must not hide the error");
        let error = String::from_utf8_lossy(&output.stderr);
        for expected in [
            args[0],
            "requires --exec <COMMAND>, --use <INDEX>, or -- <COMMAND>",
            "Example:",
            "--tui",
        ] {
            assert!(
                error.contains(expected),
                "{args:?}: missing {expected:?} in {error}"
            );
        }
    }
    assert!(!dir.path().join(".watchlist").exists());
}

#[test]
fn invalid_values_report_the_option_value_and_correction() {
    let dir = tempfile::tempdir().unwrap();
    for (flag, value, expected) in [
        ("--max-wait", "many", "milliseconds"),
        ("--timeout", "0", "from 1"),
        ("--poll", "-1", "milliseconds"),
        ("--debounce", "31536000001", "one year"),
        ("--policy", "parallel", "queue, restart, or ignore"),
        ("--include", "[", "glob"),
        ("--use", "abc", "zero-based"),
        ("--delete", "-1", "--list"),
    ] {
        let output = app(dir.path()).args([flag, value]).output().unwrap();
        assert!(!output.status.success());
        let error = String::from_utf8_lossy(&output.stderr);
        for text in [flag, value, expected] {
            assert!(error.contains(text), "{error}");
        }
        assert!(output.stdout.is_empty());
    }
    for args in [
        vec!["--max-wait"],
        vec!["--path", "--list"],
        vec!["--use", "--save"],
    ] {
        let output = app(dir.path()).args(args).output().unwrap();
        assert!(!output.status.success());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(
            error.contains("requires a value immediately after it"),
            "{error}"
        );
        assert!(error.contains("Example:"), "{error}");
    }
}

#[test]
fn management_conflicts_identify_the_ignored_option_and_do_not_delete() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".watchlist");
    fs::write(&path, "project\\0pwd\\0\n").unwrap();
    let original = fs::read(&path).unwrap();
    for (args, action, conflict) in [
        (
            vec!["--delete", "0", "--max-wait", "4000"],
            "--delete",
            "--max-wait",
        ),
        (vec!["--list", "--path", "."], "--list", "--path"),
        (vec!["--tui", "--debounce", "200"], "--tui", "--debounce"),
        (vec!["--list", "--use", "0"], "--list", "--use"),
    ] {
        let output = app(dir.path()).args(args).output().unwrap();
        assert!(!output.status.success());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(
            error.contains(&format!("{action} cannot be combined with {conflict}")),
            "{error}"
        );
        assert_eq!(fs::read(&path).unwrap(), original);
    }
}

#[test]
fn no_arguments_and_help_explain_actions_requirements_and_order() {
    let dir = tempfile::tempdir().unwrap();
    for args in [vec![], vec!["--help"]] {
        let output = app(dir.path()).args(args).output().unwrap();
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        let help = String::from_utf8_lossy(&output.stdout);
        for text in [
            "START WATCHING",
            "STANDALONE ACTIONS",
            "WATCH OPTIONS - require",
            "These options cannot be used alone",
            "Order does not matter",
            "watchlist --use 0 --max-wait 4000",
        ] {
            assert!(help.contains(text), "missing {text:?}");
        }
    }
}

#[test]
fn nonexistent_paths_and_saved_indices_report_the_relevant_flag() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing-directory");
    for flag in ["--path", "--workdir"] {
        let output = app(dir.path())
            .args(["--exec", "unused"])
            .arg(flag)
            .arg(&missing)
            .output()
            .unwrap();
        assert!(!output.status.success());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains(flag), "{error}");
        assert!(error.contains("missing-directory"), "{error}");
        assert!(error.contains("existing directory"), "{error}");
    }
    let output = app(dir.path()).args(["--use", "7"]).output().unwrap();
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(error.contains("invalid --use index 7"));
    assert!(error.contains("no saved records exist"));
    assert!(error.contains("--list"));
    fs::write(dir.path().join(".watchlist"), "project\\0pwd\\0\n").unwrap();
    let output = app(dir.path()).args(["--use", "7"]).output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("valid indices are 0 through 0"));
}
