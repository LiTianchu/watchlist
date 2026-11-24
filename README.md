# Watchlist

Watch a directory, run a command on changes, and manage saved configurations from the CLI or Ratatui editor. Requires Rust 1.89+.

```sh
cargo build --release
# Binary: target/release/watchlist (watchlist.exe on Windows)
```

## CLI commands

```sh
watchlist --help                                   # Show all options
watchlist -p . -e "npm run build"                   # Watch and run on changes
watchlist -p . -e "npm run build" --save            # Save and start watching
watchlist -p . -e "npm run build" --label "Build" -s # Save with an optional nickname
watchlist --list                                   # List saved records
watchlist --use 0                                  # Watch using record 0
watchlist --delete 0                               # Delete record 0
watchlist --tui                                    # Open configuration editor
watchlist --tui --use 0                            # Select record 0 in the editor
watchlist -p . -- printf '%s\n' 'hello world'        # Pass literal command arguments
```

Existing short flags remain available: `-h` help, `-p` path, `-e` command, `-s` save, `-l` list, `-u` use, `-d` delete. From source, use `cargo run -- --tui` or pass other options after `--`.

## Watch options

```text
--include GLOB          Repeatable allowlist; replaces default file extensions
--label NAME            Optional record nickname; an empty name clears it
--exclude GLOB          Repeatable additional exclusions; exclusions win
--no-default-ignores   Disable built-in exclusions
--workdir PATH        Command working directory (default: watched directory)
--poll MS             Content-aware polling instead of native events
--debounce MS         Quiet period (default: 500 ms; zero allowed)
--max-wait MS         Maximum delay during continuous changes (default: 5000 ms)
--timeout MS          Cancel commands after this duration (default: unlimited)
--policy POLICY       queue (one pending rerun), restart, or ignore (default: queue)
--initial             Run immediately after watching starts
```

```sh
watchlist -p . --include '**/*.md' --no-default-ignores -e "make docs"
watchlist --use 0 --debounce 200 --timeout 30000 --policy restart
watchlist -p . --poll 1000 --exclude '**/generated/**' -e "npm test"
```

Quote glob patterns. Explicit options override saved values. `--exec` uses shell-style quoting; shell operators require an explicit shell, such as `-e "sh -c 'build && test'"`. Use `-- COMMAND ARG...` for literal arguments, especially Windows paths. Other durations must be positive and at most one year.

## TUI keys

- **Browser:** ↑/↓ select, **n** new, **Enter/e** edit, **c** copy, **d** delete, **r** reload, **q** quit.
- **Editor:** **Tab/Shift+Tab** or **1–4** selects General / Includes / Excludes / Runtime; ↑/↓ selects a field; **Enter** edits or toggles it. General includes an optional **Label**; leave it blank to remove it. Labels appear only when set.
- **Rules:** **a** add, **Enter/e** edit, **d** remove, **p** copy defaults into editable rules. Copying default exclusions disables automatic built-in ignores.
- **Text:** ←/→ and Home/End move the cursor, **Ctrl+U** clears, **Enter** applies, **Esc** cancels. Pasting is supported.
- **Ctrl+S** saves the draft; **Esc** goes back; **?** shows help. Unsaved changes and record deletion require confirmation. Commands never run inside the editor. Minimum size: 60×18.

Defaults retain the original extension allowlist and ignores in `src/patterns.rs`, including Markdown, lockfiles, assets, and build directories. Empty custom includes restore the default allowlist. Ctrl+C stops watching. Polling can help on WSL Windows mounts and network filesystems.

Records use the original user-data location: `dirs::data_dir()/watchlist/.watchlist`; set `WATCHLIST_DATA_DIR` to override its directory. Legacy records remain readable; new saves use JSON that older binaries cannot read. Updates are locked and atomic; stale TUI edits are rejected. Arguments are stored as plain text, so avoid credentials in them.

## Checks

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
```
