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
                    return Err("use either --exec or -- <COMMAND>, not both".into());
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
                    let value = args.next().ok_or_else(|| missing_value(flag, None))?;
                    if !matches!(flag, "-e" | "--exec" | "--label")
                        && let Some(next) = value.to_str()
                        && (next.starts_with("--")
                            || ["-h", "-p", "-e", "-s", "-l", "-d", "-u"].contains(&next))
                    {
                        return Err(missing_value(flag, Some(next)));
                    }
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
                                return Err("--exec may only be provided once; put the executable and all its arguments in one quoted value".into());
                            }
                            cli.command = Some(
                                shell_words::split(&value)
                                    .map_err(|e| format!("invalid --exec quoting: {e}; close all quotes, for example --exec \"printf 'hello world'\""))?,
                            );
                        }
                        "-u" | "--use" => cli.use_index = Some(index(&value, flag)?),
                        "-d" | "--delete" => cli.delete = Some(index(&value, flag)?),
                        "--label" => cli.label = Some(value),
                        _ => cli.overrides.push((flag.into(), value)),
                    }
                }
                _ if !flag.starts_with('-') => {
                    return Err(format!(
                        "unexpected argument {flag:?}; commands must follow --exec or --.\nExample: watchlist -- {flag}\nUse --help to see all options."
                    ));
                }
                _ => {
                    return Err(format!(
                        "unknown option {flag:?}; use --help to see valid options"
                    ));
                }
            }
        }
        if cli.help {
            return Ok(cli);
        }
        let watch_options = cli.watch_option_names();
        if cli.tui {
            let conflict = if cli.list {
                Some("--list")
            } else if cli.delete.is_some() {
                Some("--delete")
            } else if cli.command.is_some() {
                Some("--exec / -- <COMMAND>")
            } else {
                watch_options.first().copied()
            };
            if let Some(conflict) = conflict {
                return Err(format!(
                    "--tui cannot be combined with {conflict}; it only accepts --use <INDEX> to select a record (or --help).\nEdit watch settings inside the TUI. Example: watchlist --tui --use 0"
                ));
            }
        }
        if let Some(command) = &cli.command
            && command.first().is_none_or(|s| s.is_empty())
        {
            return Err("no executable was supplied; --exec needs a nonempty command, and -- needs an executable after it.\nExamples: watchlist --exec \"cargo check\"\n          watchlist -- cargo check".into());
        }
        if cli.list || cli.delete.is_some() {
            let conflict = if cli.command.is_some() {
                Some("--exec / -- <COMMAND>")
            } else if cli.use_index.is_some() {
                Some("--use")
            } else {
                watch_options.first().copied()
            };
            if let Some(conflict) = conflict {
                let action = if cli.list { "--list" } else { "--delete" };
                return Err(format!(
                    "{action} cannot be combined with {conflict}; listing/deleting records does not start a watch session.\nUse --list or --delete <INDEX> without watch options (they may be combined with each other).\nTo apply watch options, select --use <INDEX> or --exec <COMMAND> instead."
                ));
            }
        }
        // Validate options before any storage operation.
        cli.apply_options(&mut WatchOptions::default())?;
        if cli.command.is_none() && cli.use_index.is_none() && !watch_options.is_empty() {
            return Err(format!(
                "{} requires --exec <COMMAND>, --use <INDEX>, or -- <COMMAND> to select a watch session; no command or saved record was supplied.\nExample: {}\nOptions may appear before or after --exec/--use; all Watchlist options must precede -- <COMMAND>.\nTo edit saved settings without watching, use watchlist --tui.",
                watch_options.join(", "),
                example(watch_options[0])
            ));
        }
        Ok(cli)
    }
    fn watch_option_names(&self) -> Vec<&str> {
        [
            self.path.as_ref().map(|_| "--path"),
            self.save.then_some("--save"),
            self.label.as_ref().map(|_| "--label"),
        ]
        .into_iter()
        .flatten()
        .chain(self.overrides.iter().map(|(flag, _)| flag.as_str()))
        .collect()
    }
    pub fn apply_options(&self, options: &mut WatchOptions) -> Result<(), String> {
        let mut includes = false;
        let mut excludes = false;
        for (flag, value) in &self.overrides {
            match flag.as_str() {
                "--include" => {
                    validate_pattern(value, flag)?;
                    if !includes {
                        options.includes.clear();
                        includes = true;
                    }
                    options.includes.push(value.clone());
                }
                "--exclude" => {
                    validate_pattern(value, flag)?;
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
                        _ => {
                            return Err(format!(
                                "invalid value {value:?} for --policy; choose queue, restart, or ignore.\nExample: {}",
                                example(flag)
                            ));
                        }
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
        .map_err(|_| format!("invalid value {value:?} for {flag}; expected a zero-based nonnegative record index (0, 1, 2, ...).\nUse watchlist --list to see record indices."))
}
fn number(value: &str, flag: &str) -> Result<u64, String> {
    let minimum = if flag == "--debounce" { 0 } else { 1 };
    let value_number = value
        .parse::<u64>()
        .ok()
        .filter(|ms| (minimum..=31_536_000_000).contains(ms));
    value_number.ok_or_else(|| format!("invalid value {value:?} for {flag}; expected milliseconds as an integer from {minimum} to 31536000000 (one year).\nExample: {}", example(flag)))
}

fn validate_pattern(value: &str, flag: &str) -> Result<(), String> {
    glob::Pattern::new(value).map(|_| ()).map_err(|e| {
        format!(
            "invalid glob {value:?} for {flag}: {e}.\nQuote glob patterns. Example: {}",
            example(flag)
        )
    })
}

fn example(flag: &str) -> &'static str {
    match flag {
        "-p" | "--path" => "watchlist --exec \"cargo check\" --path .",
        "-e" | "--exec" => "watchlist --exec \"cargo check\"",
        "-u" | "--use" => "watchlist --use 0",
        "-d" | "--delete" => "watchlist --delete 0",
        "-s" | "--save" => "watchlist --exec \"cargo check\" --save",
        "--label" => "watchlist --exec \"cargo check\" --label \"Build\" --save",
        "--include" => "watchlist --use 0 --include '**/*.rs'",
        "--exclude" => "watchlist --use 0 --exclude '**/generated/**'",
        "--no-default-ignores" => "watchlist --use 0 --no-default-ignores",
        "--workdir" => "watchlist --use 0 --workdir .",
        "--poll" => "watchlist --use 0 --poll 1000",
        "--debounce" => "watchlist --use 0 --debounce 500",
        "--max-wait" => "watchlist --use 0 --max-wait 4000",
        "--timeout" => "watchlist --use 0 --timeout 30000",
        "--policy" => "watchlist --use 0 --policy queue",
        "--initial" => "watchlist --use 0 --initial",
        _ => unreachable!(),
    }
}

fn missing_value(flag: &str, next: Option<&str>) -> String {
    let received = next
        .map(|next| format!("; got option {next:?} instead"))
        .unwrap_or_default();
    format!(
        "{flag} requires a value immediately after it{received}.\nExample: {}",
        example(flag)
    )
}

pub const HELP: &str = "Watch a directory and run a command on change.

Replace <...> placeholders with values; do not type the angle brackets.
Square brackets [...] mark optional parts; examples below use actual values.

START WATCHING - choose a command or saved record:
  watchlist --exec \"<COMMAND>\" [<WATCH_OPTIONS>...]
  watchlist --use <INDEX> [<WATCH_OPTIONS>...]
  watchlist [<WATCH_OPTIONS>...] -- <COMMAND> [<ARG>...]

  -e, --exec <COMMAND>            Command with shell-style quoting; no implicit shell
  -u, --use <INDEX>               Load a saved record and start watching
  -- <COMMAND> [<ARG>...]         Literal executable/arguments; all Watchlist flags go before --

STANDALONE ACTIONS - no watch command required:
  -h, --help                      Show this help; no records are changed
  -l, --list                      List saved records with zero-based indices
  -d, --delete <INDEX>            Delete a record; may combine with --list
      --tui                       Edit saved settings interactively; may combine with --use <INDEX>

WATCH OPTIONS - require --exec <COMMAND>, --use <INDEX>, or -- <COMMAND>:
  These options cannot be used alone or with --list/--delete/--tui.
  Order does not matter with --exec/--use; put all options before -- <COMMAND>.

  -p, --path <PATH>               Directory to watch (default: current directory)
  -s, --save                      Save this configuration AND start watching
      --label <NAME>              Optional nickname; an empty name clears it
      --include <GLOB>            Repeatable allowlist; replaces default extension patterns
      --exclude <GLOB>            Repeatable exclusions; exclusions always win
      --no-default-ignores Disable built-in exclusions
      --workdir <PATH>            Command working directory (default: watched directory)
      --poll <MS>                 Content-aware polling interval (default: native events)
      --debounce <MS>             Quiet period (default: 500; zero allowed)
      --max-wait <MS>             Limit delay during continuous changes (default: 5000)
      --timeout <MS>              Cancel a command after this duration (default: unlimited)
      --policy <POLICY>           queue (default), restart, or ignore changes while running
      --initial                   Run once after watching starts

  <MS> values are milliseconds, positive except --debounce, and at most one year.
  Watch options change this session. --save stores the resulting configuration.
  To edit an existing saved record without watching, use --tui.

Examples:
  watchlist --use 0 --max-wait 4000
  watchlist --exec \"npm run build\" --path . --save
  watchlist --max-wait 4000 --exec \"cargo check\"
  watchlist --path . -- printf '%s\\n' 'hello world'
  watchlist --tui --use 0
";

#[cfg(test)]
mod tests {
    use super::*;
    fn parse(args: &[&str]) -> Result<Cli, String> {
        Cli::parse(args.iter().map(OsString::from))
    }
    #[test]
    fn watch_options_work_in_either_order_with_a_session_selector() {
        for args in [
            vec!["--max-wait", "4000", "--exec", "cargo check"],
            vec!["--exec", "cargo check", "--max-wait", "4000"],
            vec!["--max-wait", "4000", "--use", "0"],
            vec!["--use", "0", "--max-wait", "4000"],
            vec!["--max-wait", "4000", "--", "cargo", "check"],
        ] {
            let cli = parse(&args).unwrap();
            let mut options = WatchOptions::default();
            cli.apply_options(&mut options).unwrap();
            assert_eq!(options.max_wait_ms, 4000);
        }
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
