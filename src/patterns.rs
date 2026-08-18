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
