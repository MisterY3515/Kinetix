/// Terminal module — ANSI terminal control + bash-like shell commands.
/// Accessible via `term.X()` in Kinetix scripts and used by `kivm shell`.

use crate::vm::Value;
use std::io::Write;

/// Map color names to ANSI codes.
fn ansi_color(name: &str) -> &'static str {
    match name {
        "black"   => "\x1b[30m",
        "red"     => "\x1b[31m",
        "green"   => "\x1b[32m",
        "yellow"  => "\x1b[33m",
        "blue"    => "\x1b[34m",
        "magenta" => "\x1b[35m",
        "cyan"    => "\x1b[36m",
        "white"   => "\x1b[37m",
        "reset"   => "\x1b[0m",
        // Bright variants
        "bright_red"     => "\x1b[91m",
        "bright_green"   => "\x1b[92m",
        "bright_yellow"  => "\x1b[93m",
        "bright_blue"    => "\x1b[94m",
        "bright_magenta" => "\x1b[95m",
        "bright_cyan"    => "\x1b[96m",
        "bright_white"   => "\x1b[97m",
        _ => "\x1b[0m",
    }
}

pub fn call(func_name: &str, args: &[Value]) -> Result<Value, String> {
    match func_name {
        // ── ANSI Terminal Control ──

        "clear" => {
            print!("\x1b[2J\x1b[H");
            std::io::stdout().flush().ok();
            Ok(Value::Null)
        }
        "set_color" => {
            let color = match args.first() {
                Some(Value::Str(s)) => s.as_str(),
                _ => "reset",
            };
            print!("{}", ansi_color(color));
            std::io::stdout().flush().ok();
            Ok(Value::Null)
        }
        "reset_color" => {
            print!("\x1b[0m");
            std::io::stdout().flush().ok();
            Ok(Value::Null)
        }
        "bold" => {
            let text = args.first().map(|v| format!("{}", v)).unwrap_or_default();
            Ok(Value::Str(format!("\x1b[1m{}\x1b[22m", text)))
        }
        "underline" => {
            let text = args.first().map(|v| format!("{}", v)).unwrap_or_default();
            Ok(Value::Str(format!("\x1b[4m{}\x1b[24m", text)))
        }
        "italic" => {
            let text = args.first().map(|v| format!("{}", v)).unwrap_or_default();
            Ok(Value::Str(format!("\x1b[3m{}\x1b[23m", text)))
        }
        "strikethrough" => {
            let text = args.first().map(|v| format!("{}", v)).unwrap_or_default();
            Ok(Value::Str(format!("\x1b[9m{}\x1b[29m", text)))
        }
        "color_print" => {
            let color = match args.first() {
                Some(Value::Str(s)) => s.clone(),
                _ => "white".to_string(),
            };
            let text = args.get(1).map(|v| format!("{}", v)).unwrap_or_default();
            println!("{}{}\x1b[0m", ansi_color(&color), text);
            Ok(Value::Null)
        }
        "move_cursor" => {
            let row = match args.first() {
                Some(Value::Int(n)) => *n,
                _ => 1,
            };
            let col = match args.get(1) {
                Some(Value::Int(n)) => *n,
                _ => 1,
            };
            print!("\x1b[{};{}H", row, col);
            std::io::stdout().flush().ok();
            Ok(Value::Null)
        }
        "hide_cursor" => {
            print!("\x1b[?25l");
            std::io::stdout().flush().ok();
            Ok(Value::Null)
        }
        "show_cursor" => {
            print!("\x1b[?25h");
            std::io::stdout().flush().ok();
            Ok(Value::Null)
        }
        "size" => {
            // Cross-platform terminal size
            let (cols, rows) = terminal_size();
            Ok(Value::Array(vec![Value::Int(cols), Value::Int(rows)]))
        }

        // ── Bash-like Commands ──

        "pwd" => {
            let cwd = std::env::current_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| "?".into());
            Ok(Value::Str(cwd))
        }
        "cd" => {
            let dir = match args.first() {
                Some(Value::Str(s)) => s.clone(),
                _ => std::env::var("HOME")
                    .or(std::env::var("USERPROFILE"))
                    .unwrap_or_else(|_| ".".into()),
            };
            std::env::set_current_dir(&dir)
                .map_err(|e| format!("cd: {}: {}", dir, e))?;
            Ok(Value::Null)
        }
        "ls" => {
            let dir = match args.first() {
                Some(Value::Str(s)) => s.clone(),
                _ => ".".to_string(),
            };
            let entries = std::fs::read_dir(&dir)
                .map_err(|e| format!("ls: {}: {}", dir, e))?;
            let mut names = Vec::new();
            for entry in entries {
                if let Ok(e) = entry {
                    names.push(Value::Str(e.file_name().to_string_lossy().to_string()));
                }
            }
            Ok(Value::Array(names))
        }
        "cat" => {
            let path = match args.first() {
                Some(Value::Str(s)) => s.clone(),
                _ => return Err("cat: missing filename".into()),
            };
            let content = std::fs::read_to_string(&path)
                .map_err(|e| format!("cat: {}: {}", path, e))?;
            Ok(Value::Str(content))
        }
        "mkdir" => {
            let path = match args.first() {
                Some(Value::Str(s)) => s.clone(),
                _ => return Err("mkdir: missing directory name".into()),
            };
            std::fs::create_dir_all(&path)
                .map_err(|e| format!("mkdir: {}: {}", path, e))?;
            Ok(Value::Null)
        }
        "rm" => {
            let path = match args.first() {
                Some(Value::Str(s)) => s.clone(),
                _ => return Err("rm: missing filename".into()),
            };
            let p = std::path::Path::new(&path);
            if p.is_dir() {
                std::fs::remove_dir_all(&path)
                    .map_err(|e| format!("rm: {}: {}", path, e))?;
            } else {
                std::fs::remove_file(&path)
                    .map_err(|e| format!("rm: {}: {}", path, e))?;
            }
            Ok(Value::Null)
        }
        "cp" => {
            let src = match args.first() {
                Some(Value::Str(s)) => s.clone(),
                _ => return Err("cp: missing source".into()),
            };
            let dst = match args.get(1) {
                Some(Value::Str(s)) => s.clone(),
                _ => return Err("cp: missing destination".into()),
            };
            std::fs::copy(&src, &dst)
                .map_err(|e| format!("cp: {} -> {}: {}", src, dst, e))?;
            Ok(Value::Null)
        }
        "mv" => {
            let src = match args.first() {
                Some(Value::Str(s)) => s.clone(),
                _ => return Err("mv: missing source".into()),
            };
            let dst = match args.get(1) {
                Some(Value::Str(s)) => s.clone(),
                _ => return Err("mv: missing destination".into()),
            };
            std::fs::rename(&src, &dst)
                .map_err(|e| format!("mv: {} -> {}: {}", src, dst, e))?;
            Ok(Value::Null)
        }
        "echo" => {
            let text: Vec<String> = args.iter().map(|v| format!("{}", v)).collect();
            let line = text.join(" ");
            println!("{}", line);
            Ok(Value::Str(line))
        }
        "touch" => {
            let path = match args.first() {
                Some(Value::Str(s)) => s.clone(),
                _ => return Err("touch: missing filename".into()),
            };
            if !std::path::Path::new(&path).exists() {
                std::fs::File::create(&path)
                    .map_err(|e| format!("touch: {}: {}", path, e))?;
            }
            Ok(Value::Null)
        }
        "which" => {
            let cmd = match args.first() {
                Some(Value::Str(s)) => s.clone(),
                _ => return Err("which: missing command".into()),
            };
            // Search PATH for the command
            if let Ok(path_var) = std::env::var("PATH") {
                let sep = if cfg!(target_os = "windows") { ';' } else { ':' };
                let exts: Vec<&str> = if cfg!(target_os = "windows") {
                    vec![".exe", ".cmd", ".bat", ".com"]
                } else {
                    vec![""]
                };
                for dir in path_var.split(sep) {
                    for ext in &exts {
                        let candidate = std::path::Path::new(dir).join(format!("{}{}", cmd, ext));
                        if candidate.exists() {
                            return Ok(Value::Str(candidate.to_string_lossy().to_string()));
                        }
                    }
                }
            }
            Ok(Value::Null)
        }
        "whoami" => {
            let user = std::env::var("USERNAME")
                .or(std::env::var("USER"))
                .unwrap_or_else(|_| "unknown".into());
            Ok(Value::Str(user))
        }
        "env" => {
            // Return all environment variables as a map
            let mut map = std::collections::HashMap::new();
            for (key, val) in std::env::vars() {
                map.insert(key, Value::Str(val));
            }
            Ok(Value::Map(map))
        }
        "head" => {
            let path = match args.first() {
                Some(Value::Str(s)) => s.clone(),
                _ => return Err("head: missing filename".into()),
            };
            let n = match args.get(1) {
                Some(Value::Int(n)) => *n as usize,
                _ => 10,
            };
            let content = std::fs::read_to_string(&path)
                .map_err(|e| format!("head: {}: {}", path, e))?;
            let lines: Vec<&str> = content.lines().take(n).collect();
            Ok(Value::Str(lines.join("\n")))
        }
        "tail" => {
            let path = match args.first() {
                Some(Value::Str(s)) => s.clone(),
                _ => return Err("tail: missing filename".into()),
            };
            let n = match args.get(1) {
                Some(Value::Int(n)) => *n as usize,
                _ => 10,
            };
            let content = std::fs::read_to_string(&path)
                .map_err(|e| format!("tail: {}: {}", path, e))?;
            let all_lines: Vec<&str> = content.lines().collect();
            let start = all_lines.len().saturating_sub(n);
            Ok(Value::Str(all_lines[start..].join("\n")))
        }
        "wc" => {
            let path = match args.first() {
                Some(Value::Str(s)) => s.clone(),
                _ => return Err("wc: missing filename".into()),
            };
            let content = std::fs::read_to_string(&path)
                .map_err(|e| format!("wc: {}: {}", path, e))?;
            let lines = content.lines().count() as i64;
            let words = content.split_whitespace().count() as i64;
            let chars = content.len() as i64;
            Ok(Value::Map(std::collections::HashMap::from([
                ("lines".to_string(), Value::Int(lines)),
                ("words".to_string(), Value::Int(words)),
                ("bytes".to_string(), Value::Int(chars)),
            ])))
        }
        "grep" => {
            let pattern = match args.first() {
                Some(Value::Str(s)) => s.clone(),
                _ => return Err("grep: missing pattern".into()),
            };
            let path = match args.get(1) {
                Some(Value::Str(s)) => s.clone(),
                _ => return Err("grep: missing filename".into()),
            };
            let content = std::fs::read_to_string(&path)
                .map_err(|e| format!("grep: {}: {}", path, e))?;
            let matches: Vec<Value> = content
                .lines()
                .filter(|line| line.contains(&pattern))
                .map(|line| Value::Str(line.to_string()))
                .collect();
            Ok(Value::Array(matches))
        }

        _ => Err(format!("Unknown term function: {}", func_name)),
    }
}

/// Get terminal size (cross-platform fallback).
fn terminal_size() -> (i64, i64) {
    // Try environment variables first (works in most terminals)
    if let (Ok(cols), Ok(rows)) = (std::env::var("COLUMNS"), std::env::var("LINES")) {
        if let (Ok(c), Ok(r)) = (cols.parse::<i64>(), rows.parse::<i64>()) {
            return (c, r);
        }
    }
    // Default fallback
    (80, 24)
}
