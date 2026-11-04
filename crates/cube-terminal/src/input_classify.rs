//! 判断输入是 shell 命令还是自然语言。
//!
//! 仅使用结构信号，不依赖静态词表：
//! 1. 首个 token 是 $PATH 上的可执行文件或 shell 内建
//! 2. 首个 token 是路径（`./`、`/`、`~/`）
//! 3. 输入包含 shell 操作符（`|`、`>`、`>>`、`&&`、`;`）
//! 4. 首个 token 形如命令且带有 flag 参数（`-x`、`--foo`）
//! 5. 首个 token 是环境变量赋值（`VAR=value`）
//! 6. 输入包含子 shell/展开语法（`$(...)`、反引号）
//!
//! `is_remote` 为真时（聚焦 pane 是 SSH 会话），远端可执行文件不在本地
//! `$PATH` 上，因此只凭命令形状的首词即可判定；自然语言信号会覆盖该判断。

use std::collections::HashSet;
use std::sync::OnceLock;

const SHELL_BUILTINS: &[&str] = &[
    // POSIX required builtins
    "cd", "echo", "eval", "exec", "exit", "export", "readonly", "return", "set", "shift", "source",
    "test", "times", "trap", "type", "ulimit", "umask", "unset", "wait",
    // bash/zsh builtins commonly typed as commands
    "alias", "bg", "bind", "builtin", "caller", "command", "compgen", "complete", "declare", "dirs",
    "disown", "enable", "fc", "fg", "hash", "help", "history", "jobs", "let", "local", "logout",
    "popd", "pushd", "read", "shopt", "suspend", "typeset", "unalias",
];

/// Scan $PATH once and cache the set of executable names.
fn path_executables() -> &'static HashSet<String> {
    static CACHE: OnceLock<HashSet<String>> = OnceLock::new();
    CACHE.get_or_init(|| {
        let path_var = std::env::var("PATH").unwrap_or_default();
        let mut exes = HashSet::with_capacity(2048);
        for dir in std::env::split_paths(&path_var) {
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for entry in entries.filter_map(|e| e.ok()) {
                    if let Some(name) = entry.file_name().to_str() {
                        exes.insert(name.to_string());
                    }
                }
            }
        }
        exes
    })
}

/// Discover shell aliases by running `alias` in the user's login shell.
/// Cached on first call. Returns empty set on failure (no blocking, no panic).
fn shell_aliases() -> &'static HashSet<String> {
    static CACHE: OnceLock<HashSet<String>> = OnceLock::new();
    CACHE.get_or_init(|| {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into());
        let output = cube_paths::host_command(&shell)
            .args(["-ic", "alias"])
            .stderr(std::process::Stdio::null())
            .output()
            .ok();
        let mut aliases = HashSet::new();
        if let Some(out) = output {
            let text = String::from_utf8_lossy(&out.stdout);
            for line in text.lines() {
                // zsh: "name=value" or "name='value'"
                // bash: "alias name='value'"
                let line = line.trim();
                let name_part = if let Some(rest) = line.strip_prefix("alias ") {
                    rest
                } else {
                    line
                };
                if let Some((name, _)) = name_part.split_once('=') {
                    let name = name.trim();
                    if !name.is_empty() && is_command_shaped(name) {
                        aliases.insert(name.to_string());
                    }
                }
            }
        }
        log::info!("[classify] discovered {} shell aliases", aliases.len());
        aliases
    })
}

/// Returns true if a token looks like a valid command name:
/// lowercase alphanumeric, hyphens, underscores, dots — no spaces.
fn is_command_shaped(word: &str) -> bool {
    !word.is_empty()
        && word.len() <= 40
        && word.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_' || b == b'.'
        })
}

/// Detect whether input is a shell command or natural language.
///
/// Uses structural signals — no static word list:
/// 1. First token is an executable on $PATH or a shell builtin
/// 2. First token is a path (`./`, `/`, `~/`)
/// 3. Input contains shell operators (`|`, `>`, `>>`, `&&`, `;`)
/// 4. Input contains flag-like arguments (`-x`, `--foo`) with a command-shaped first word
/// 5. First token is an env var assignment (`VAR=value`)
/// 6. Input contains subshell/expansion syntax (`$(...)`, backticks)
///
/// When `is_remote` is true (focused pane is an SSH session), the classification
/// is more permissive: a command-shaped first word alone is enough, since remote
/// executables aren't on the local $PATH. Natural-language signals (question words,
/// articles, pronouns) override this to prevent false positives.
pub fn looks_like_command(input: &str, is_remote: bool) -> bool {
    let mut words = input.split_whitespace();
    let first_word = match words.next() {
        Some(w) => w,
        None => return false,
    };

    // --- Definitive structural signals (always apply) ---

    // Shell builtins (these are never NL — "cd", "export", etc.)
    if SHELL_BUILTINS.contains(&first_word) {
        return true;
    }

    // Shell aliases (ll, la, gs, gst, etc.) — discovered from user's shell config
    if shell_aliases().contains(first_word) {
        return true;
    }

    // Executable on local $PATH — but only if the surrounding text
    // doesn't read like natural language. Many short English words
    // (`what`, `test`, `time`, `sort`, `make`) are also executables.
    if path_executables().contains(first_word) && !has_natural_language_signals(input) {
        return true;
    }

    // Explicit path invocation
    if first_word.starts_with("./") || first_word.starts_with('/') || first_word.starts_with("~/") {
        return true;
    }

    // Env var assignment: VAR=value or VAR=value command
    if first_word.contains('=') {
        let (name, _) = first_word.split_once('=').unwrap();
        if !name.is_empty()
            && name
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
        {
            return true;
        }
    }

    // Shell operators indicate a pipeline/compound command
    if input.contains(" | ")
        || input.contains(" > ")
        || input.contains(" >> ")
        || input.contains(" && ")
        || input.contains(" ; ")
    {
        return true;
    }

    // Subshell / expansion syntax
    if input.contains("$(") || input.contains('`') {
        return true;
    }

    // Flag arguments with a command-shaped first word:
    // "free -g", "docker --version" — NL never uses -flags
    if is_command_shaped(first_word) {
        let has_flags = words.clone().any(|w| w.starts_with('-'));
        if has_flags {
            return true;
        }
    }

    // --- Remote-aware classification ---
    // On SSH sessions, remote executables aren't on local $PATH.
    // A command-shaped first word is likely a remote command — unless
    // the input reads like natural language.
    if is_remote && is_command_shaped(first_word) && !has_natural_language_signals(input) {
        return true;
    }

    false
}

/// Detects natural-language syntax using structural signals only.
///
/// Uses three purely syntactic checks — no word lists:
/// 1. Sentence-ending punctuation: `?` or `.` at the end (shell commands never end this way)
/// 2. First word starts with uppercase (shell executables are lowercase)
/// 3. Contains clause-separating commas (`, ` — distinct from shell comma usage)
/// 4. Multi-word input with no shell argument structure (no flags, paths, or operators)
fn has_natural_language_signals(input: &str) -> bool {
    let trimmed = input.trim();

    // 1. Ends with sentence punctuation — commands never end with ? or .
    //    (note: shell `!` is history expansion, so we skip it)
    if trimmed.ends_with('?') || trimmed.ends_with('.') {
        return true;
    }

    // 2. First character is uppercase — shell commands are lowercase.
    //    Env var assignments (VAR=val) are caught earlier in the pipeline.
    if trimmed.starts_with(|c: char| c.is_ascii_uppercase()) {
        return true;
    }

    // 3. Contains clause-separating comma+space pattern.
    //    Shell uses commas in brace expansion {a,b} but not ", " between words.
    if trimmed.contains(", ") {
        return true;
    }

    // 4. Multi-word input where no token looks like a shell argument.
    //    Shell arguments are: flags (-x, --foo), paths (/foo, ./bar),
    //    env vars ($FOO), globs (*.txt), or quoted strings.
    //    If none of the tokens after the first have these patterns, it's likely prose.
    let words: Vec<&str> = trimmed.split_whitespace().collect();
    if words.len() >= 3 {
        let has_shell_args = words[1..].iter().any(|w| {
            w.starts_with('-')       // flag
                || w.starts_with('/')    // absolute path
                || w.starts_with("./")   // relative path
                || w.starts_with("~/")   // home path
                || w.starts_with('$')    // variable
                || w.contains('*')       // glob
                || w.contains('=')       // assignment
                || w.starts_with('"')    // quoted string
                || w.starts_with('\'') // quoted string
        });
        if !has_shell_args {
            return true;
        }
    }

    false
}
