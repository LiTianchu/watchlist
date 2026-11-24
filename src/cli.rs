use crate::config::{ExecutionPolicy, WatchOptions};
use std::{ffi::OsString, path::PathBuf};

#[derive(Debug, Default)]
pub struct Cli {
    pub help: bool,
    pub tui: bool,
    pub list: bool,
    pub delete: Option<usize>,
    pub use_index: Option<usize>,
    pub save: bool,
    pub path: Option<PathBuf>,
    pub command: Option<Vec<String>>,
    pub label: Option<String>,
    overrides: Vec<(String, String)>,
}
impl Cli {
    pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Self, String> {
        let mut cli = Self::default();
        let mut args = args.into_iter();
        while let Some(raw) = args.next() {
            let flag = raw.to_str().ok_or("option must be UTF-8")?;
            if flag == "--" {
                let command = args
                    .map(|v| {
                        v.into_string()
                            .map_err(|_| "command arguments must be UTF-8".to_string())
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                if cli.command.is_some() {
                    return Err("use either --exec or -- COMMAND, not both".into());
                }
                cli.command = Some(command);
                break;
            }
            match flag {
                "-h" | "--help" => cli.help = true,
                "--tui" => cli.tui = true,
                "-s" | "--save" => cli.save = true,
                "-l" | "--list" => cli.list = true,
                "--initial" | "--no-default-ignores" => {
                    cli.overrides.push((flag.into(), String::new()))
                }
                "-p" | "--path" | "-e" | "--exec" | "-u" | "--use" | "-d" | "--delete"
                | "--include" | "--exclude" | "--workdir" | "--poll" | "--debounce"
                | "--max-wait" | "--timeout" | "--policy" | "--label" => {
                    let value = args
                        .next()
                        .ok_or_else(|| format!("{flag} requires a value"))?;
                    if matches!(flag, "-p" | "--path") {
                        cli.path = Some(value.into());
                        continue;
                    }
                    // Preserve native paths for --path; persisted command strings use UTF-8.
                    let value = value
                        .into_string()
                        .map_err(|_| format!("{flag} requires UTF-8"))?;
                    match flag {
                        "-e" | "--exec" => {
                            if cli.command.is_some() {
                                return Err("--exec may only be provided once".into());
                            }
                            cli.command = Some(
                                shell_words::split(&value)
                                    .map_err(|e| format!("invalid --exec quoting: {e}"))?,
                            );
                        }
                        "-u" | "--use" => cli.use_index = Some(index(&value, flag)?),
                        "-d" | "--delete" => cli.delete = Some(index(&value, flag)?),
                        "--label" => cli.label = Some(value),
                        _ => cli.overrides.push((flag.into(), value)),
                    }
                }
                _ => return Err(format!("unknown option {flag:?}; use --help")),
            }
        }
        if cli.help {
            return Ok(cli);
        }
        if cli.tui
            && (cli.list
                || cli.delete.is_some()
                || cli.save
                || cli.path.is_some()
                || cli.command.is_some()
                || cli.label.is_some()
                || !cli.overrides.is_empty())
        {
            return Err("--tui may only be combined with --use INDEX or --help".into());
        }
        if let Some(command) = &cli.command
            && command.first().is_none_or(|s| s.is_empty())
        {
            return Err("an executable is required".into());
        }
        if (cli.list || cli.delete.is_some())
            && (cli.command.is_some() || cli.use_index.is_some() || cli.save || cli.label.is_some())
        {
            return Err("--list/--delete cannot be combined with a watch command".into());
        }
        // Validate options before any storage operation.
        cli.apply_options(&mut WatchOptions::default())?;
        Ok(cli)
    }
    pub fn apply_options(&self, options: &mut WatchOptions) -> Result<(), String> {
        let mut includes = false;
        let mut excludes = false;
        for (flag, value) in &self.overrides {
            match flag.as_str() {
                "--include" => {
                    if !includes {
                        options.includes.clear();
                        includes = true;
                    }
                    options.includes.push(value.clone());
                }
                "--exclude" => {
                    if !excludes {
                        options.excludes.clear();
                        excludes = true;
                    }
                    options.excludes.push(value.clone());
                }
                "--no-default-ignores" => options.default_ignores = false,
                "--initial" => options.initial = true,
                "--workdir" => options.workdir = Some(value.into()),
                "--poll" => options.poll_ms = Some(number(value, flag)?),
                "--debounce" => options.debounce_ms = number(value, flag)?,
                "--max-wait" => options.max_wait_ms = number(value, flag)?,
                "--timeout" => options.timeout_ms = Some(number(value, flag)?),
                "--policy" => {
                    options.policy = match value.as_str() {
                        "queue" => ExecutionPolicy::Queue,
                        "restart" => ExecutionPolicy::Restart,
                        "ignore" => ExecutionPolicy::Ignore,
                        _ => return Err("--policy must be queue, restart, or ignore".into()),
                    }
                }
                _ => unreachable!(),
            }
        }
        options.validate()
    }
}
fn index(value: &str, flag: &str) -> Result<usize, String> {
    value
        .parse()
        .map_err(|_| format!("{flag} requires a nonnegative index"))
}
fn number(value: &str, flag: &str) -> Result<u64, String> {
    value
        .parse()
        .map_err(|_| format!("{flag} requires milliseconds as a nonnegative integer"))
}

pub const HELP: &str = "Watch a directory and run a command on change.
Usage: watchlist [OPTIONS] --exec \"COMMAND ARGS\"
       watchlist [OPTIONS] -- COMMAND [ARG ...]
       watchlist --use INDEX [OPTIONS]
       watchlist --tui [--use INDEX]

-h, --help                 Show help without changing saved records
    --tui                 Open the interactive configuration editor
-p, --path PATH            Directory to watch (default: current directory)
-e, --exec COMMAND         Command with shell-style quoting; no implicit shell
-s, --save                 Save the configuration and start watching
    --label NAME          Optional record nickname; an empty name clears it
-l, --list                 List saved records with zero-based indices
-d, --delete INDEX         Delete a saved record (may combine with --list)
-u, --use INDEX            Load a saved configuration; explicit options override it
    --include GLOB        Repeatable allowlist; replaces default extension patterns
    --exclude GLOB        Repeatable exclusions; always take precedence
    --no-default-ignores   Disable built-in exclusions (assets, lockfiles, Markdown, etc.)
    --workdir PATH        Command working directory (default: watched directory)
    --poll MS             Use content-aware polling at this interval
    --debounce MS         Quiet period (default: 500)
    --max-wait MS         Maximum delay during continuous changes (default: 5000)
    --timeout MS          Kill a command after this duration (default: unlimited)
    --policy POLICY       queue (default), restart, or ignore changes while running
    --initial             Run once after watching starts

Examples:
  watchlist --path . --exec \"npm run build\" --save
  watchlist --use 0
  watchlist --path . -- printf '%s\\n' 'hello world'
  watchlist --include '**/*.md' --no-default-ignores --exec \"make docs\"
";

#[cfg(test)]
mod tests {
    use super::*;
    fn parse(args: &[&str]) -> Result<Cli, String> {
        Cli::parse(args.iter().map(OsString::from))
    }
    #[test]
    fn old_flags_and_quoted_args() {
        let cli = parse(&["-p", ".", "-e", "printf 'hello world' ''", "-s"]).unwrap();
        assert_eq!(cli.command.unwrap(), ["printf", "hello world", ""]);
        assert!(cli.save);
    }
    #[test]
    fn values_are_consumed() {
        let cli = parse(&["-e", "-h"]).unwrap();
        assert!(!cli.help);
        assert_eq!(cli.command.unwrap(), ["-h"]);
    }
    #[test]
    fn optional_label_is_consumed_and_can_be_cleared() {
        let cli = parse(&["-e", "program", "--label", "Build project", "--save"]).unwrap();
        assert_eq!(cli.label.as_deref(), Some("Build project"));
        assert_eq!(
            parse(&["--use", "0", "--label", ""])
                .unwrap()
                .label
                .as_deref(),
            Some("")
        );
        assert!(parse(&["--label"]).is_err());
        assert!(parse(&["--list", "--label", "name"]).is_err());
        assert!(parse(&["--tui", "--label", "name"]).is_err());
    }
    #[test]
    fn vector_command_is_literal() {
        let cli = parse(&["--", "program", "a b", "", "--help", r"C:\foo\bar"]).unwrap();
        assert!(!cli.help);
        assert_eq!(
            cli.command.unwrap(),
            ["program", "a b", "", "--help", r"C:\foo\bar"]
        );
    }
    #[test]
    fn rejects_invalid_input_without_panics() {
        for args in [
            vec!["--typo"],
            vec!["-p"],
            vec!["-e", "sh 'bad"],
            vec!["-e", ""],
            vec!["--timeout", "0"],
            vec!["--poll", "0"],
            vec!["--policy", "bad"],
            vec!["--include", "["],
            vec!["--"],
            vec!["-u", "-1"],
            vec!["-d", "0", "-s"],
        ] {
            assert!(parse(&args).is_err(), "{args:?}");
        }
    }
    #[test]
    fn help_is_a_pure_parsed_action() {
        let cli = parse(&["--help", "--delete", "0"]).unwrap();
        assert!(cli.help);
        assert_eq!(cli.delete, Some(0));
    }
    #[test]
    fn loaded_options_are_overridden_only_when_explicit() {
        let mut options = WatchOptions {
            timeout_ms: Some(200),
            includes: vec!["old".into()],
            ..WatchOptions::default()
        };
        parse(&[
            "-u",
            "0",
            "--debounce",
            "20",
            "--include",
            "*.md",
            "--include",
            "*.rs",
        ])
        .unwrap()
        .apply_options(&mut options)
        .unwrap();
        assert_eq!(options.timeout_ms, Some(200));
        assert_eq!(options.debounce_ms, 20);
        assert_eq!(options.includes, ["*.md", "*.rs"]);
    }
}
