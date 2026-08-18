# Watchlist

A simple tool for watching a directory and running a command on change. Able to save and reuse historical watch configurations.  

### Usage
```
-h | --help: Prints this help message
-p | --path <path>: Path to watch
-e | --exec <command>: Command to run on change
-s | --save: Save the current --path and --exec command to the watch record list
-l | --list: List all watch records with their indices
-d | --delete <index>: Delete a watch record by its index
-u | --use <index>: Use a watch record by its index

Example: watchlist --path . --exec "npm run build" --save
Explaination: Watches the current directory for changes and runs "npm run build" on change, and save the configuration to the watch record list

Example: watchlist --use 0
Explaination: Uses the saved watch record at index 0 to watch for changes and run the associated command,
```

### Watch and Ignore Patterns
```rust
// patterns.rs
pub const IGNORE_PATTERNS: &[&str] = &[
    // Version control
    "**/.git/**",
    "**/.svn/**",
    "**/.hg/**",
    // Rust
    "**/target/**",
    "**/Cargo.lock",
    // JS/TS/Node
    "**/node_modules/**",
    "**/.next/**",
    "**/.turbo/**",
    "**/package-lock.json",
    "**/pnpm-lock.yaml",
    "**/yarn.lock",
    // General build/output dirs
    "**/build/**",
    "**/dist/**",
    "**/out/**",
    "**/bin/**",
    "**/obj/**",       // C#/.NET
    "**/.gradle/**",   // Java/Kotlin
    "**/zig-cache/**", // Zig
    "**/zig-out/**",
    // Godot
    "**/.godot/**",
    "**/.import/**",
    // Unity
    "**/Library/**",
    "**/Temp/**",
    "**/Logs/**",
    // Python
    "**/__pycache__/**",
    "**/.venv/**",
    "**/venv/**",
    "**/*.egg-info/**",
    // Editors/tooling
    "**/.vscode/**",
    "**/.idea/**",
    "**/.zed/**",
    "**/.vs/**",
    // Assets (large/binary)
    "**/assets/**",
    "**/public/**",
    // OS/misc
    "**/.DS_Store",
    "**/Thumbs.db",
    "**/*:Zone.Identifier",
    // AI
    "**/.claude/**",
    "**/.codex/**",
    "**/.agents/**",
    // Documentation
    "**/*.md",
];

pub const WATCH_PATTERNS: &[&str] = &[
    // Systems languages
    "*.rs", "*.zig", "*.odin", "*.c", "*.cpp", "*.cc", "*.C", "*.h", "*.hpp",
    // Web / JS ecosystem
    "*.ts", "*.tsx", "*.js", "*.jsx", "*.mjs", "*.cjs", "*.vue", "*.svelte",
    // JVM / .NET / Go / scripting
    "*.java", "*.kt", "*.cs", "*.go", "*.py", "*.lua", "*.rb", "*.php",
    // Functional / logic languages
    "*.hs", "*.ml", "*.mli", "*.pl", "*.pro", "*.ex", "*.exs",
    // Game Engines (Godot, Unity)
    "*.gd", "*.tscn", "*.tres", "*.godot", "*.shader", "*.uss", "*.uxml",
    // Shaders (GLSL, HLSL, WGSL, SLANG)
    "*.glsl", "*.hlsl", "*.wgsl", "*.frag", "*.vert", "*.comp", "*.slang",
    // Markup / config / data
    "*.html", "*.css", "*.scss", "*.sass", "*.toml", "*.yaml", "*.yml", "*.json", "*.jsonc",
    // Images and asset files (for sprite pipelines)
    "*.png", "*.jpg", "*.jpeg", "*.webp", "*.svg", "*.txt",
];
```

If you need more, modify them and rebuild :)   

### Build

```
cargo build --release
```

### Debug Run
```
cargo run -- --path "<your-folder-path>" --exec "<your-command>"
```

### Notes
1. Does not work on WSL as WSL does not produce file change events for some reason
2. Not tested on Windows Shell
3. Have no security guards, if you want to execute `rm -rf` with it, good luck :)
