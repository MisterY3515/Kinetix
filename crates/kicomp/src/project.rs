/// project.rs — .kicomp Project Configuration Parser (Build 33)
///
/// Parses `.kicomp` project files into structured `ProjectConfig`.
/// The `.kicomp` format uses a brace-based declarative syntax:
///
/// ```kicomp
/// project("MyApp") {
///     version: "1.0.0"
///     entry: "src/main.kix"
///     output_type: "kivm"
///     optimize: "speed"
///     dependencies: {
///         "my_lib": "libs/my_lib"
///     }
///     sandbox: {
///         allow_network: true,
///         allow_fs_write: ["./logs"],
///         allow_audio: false
///     }
/// }
/// ```

use std::path::{Path, PathBuf};
use crate::capability::Capability;

// ─── Data Structures ─────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum OutputType {
    Kivm,
    Native,
}

#[derive(Debug, Clone, PartialEq)]
pub enum OptLevel {
    Debug,
    Speed,
    Size,
}

#[derive(Debug, Clone)]
pub struct Dependency {
    pub name: String,
    pub source: DependencySource,
}

#[derive(Debug, Clone)]
pub enum DependencySource {
    Local(PathBuf),
    // Registry(String, String), // future: url + version
}

#[derive(Debug, Clone)]
pub struct SandboxConfig {
    pub allow_network: bool,
    pub allow_fs_read: bool,
    pub allow_fs_write: Vec<String>,
    pub allow_audio: bool,
    pub allow_exec: bool,
    pub allow_threads: bool,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            allow_network: false,
            allow_fs_read: true,          // Default: reads allowed
            allow_fs_write: Vec::new(),   // Default: no write paths
            allow_audio: false,
            allow_exec: false,
            allow_threads: false,
        }
    }
}

impl SandboxConfig {
    /// Convert SandboxConfig into a list of Capability enums for the CapabilityValidator.
    pub fn to_capabilities(&self) -> Vec<Capability> {
        let mut caps = Vec::new();
        if self.allow_fs_read { caps.push(Capability::FsRead); }
        if !self.allow_fs_write.is_empty() {
            caps.push(Capability::FsRead);   // Write implies read
            caps.push(Capability::FsWrite);
        }
        if self.allow_network { caps.push(Capability::NetAccess); }
        if self.allow_exec { caps.push(Capability::OsExecute); }
        if self.allow_threads {
            caps.push(Capability::ThreadControl);
        }
        // SysInfo is always granted (non-destructive)
        caps.push(Capability::SysInfo);
        caps
    }
}

#[derive(Debug, Clone)]
pub struct ProjectConfig {
    pub name: String,
    pub version: String,
    pub author: Option<String>,
    pub entry: PathBuf,
    pub output_type: OutputType,
    pub optimize: OptLevel,
    pub dependencies: Vec<Dependency>,
    pub sandbox: SandboxConfig,
}

// ─── Parser ──────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum ProjectError {
    Io(String),
    Parse(String),
    Validation(String),
}

impl std::fmt::Display for ProjectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProjectError::Io(e) => write!(f, "IO error: {}", e),
            ProjectError::Parse(e) => write!(f, "Parse error in .kicomp: {}", e),
            ProjectError::Validation(e) => write!(f, "Validation error: {}", e),
        }
    }
}

/// Parse a `.kicomp` file at the given path into a `ProjectConfig`.
pub fn parse_kicomp(path: &Path) -> Result<ProjectConfig, ProjectError> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| ProjectError::Io(format!("Cannot read '{}': {}", path.display(), e)))?;

    parse_kicomp_str(&content, path)
}

/// Parse `.kicomp` content string.
fn parse_kicomp_str(content: &str, file_path: &Path) -> Result<ProjectConfig, ProjectError> {
    let base_dir = file_path.parent().unwrap_or(Path::new("."));

    // Strip comments
    let lines: Vec<&str> = content.lines().collect();
    let mut cleaned = String::new();
    for line in &lines {
        let trimmed = line.trim();
        if trimmed.starts_with("//") { continue; }
        // Strip inline comments
        if let Some(pos) = line.find("//") {
            cleaned.push_str(&line[..pos]);
        } else {
            cleaned.push_str(line);
        }
        cleaned.push('\n');
    }

    // Extract project name from `project("name") {`
    let name = extract_project_name(&cleaned)?;

    // Extract key-value pairs from the top-level block
    let block = extract_top_block(&cleaned)?;

    let mut version = "0.0.1".to_string();
    let mut author = None;
    let mut entry = PathBuf::from("src/main.kix");
    let mut output_type = OutputType::Kivm;
    let mut optimize = OptLevel::Debug;
    let mut dependencies = Vec::new();
    let mut sandbox = SandboxConfig::default();

    let pairs = parse_block_fields(&block)?;

    for (key, value) in &pairs {
        match key.as_str() {
            "version" => version = unquote(value)?,
            "author" => author = Some(unquote(value)?),
            "entry" => entry = PathBuf::from(unquote(value)?),
            "output_type" => {
                output_type = match unquote(value)?.as_str() {
                    "native" => OutputType::Native,
                    "kivm" | "vm" => OutputType::Kivm,
                    other => return Err(ProjectError::Parse(format!("Unknown output_type: '{}'", other))),
                };
            }
            "optimize" => {
                optimize = match unquote(value)?.as_str() {
                    "speed" => OptLevel::Speed,
                    "size" => OptLevel::Size,
                    "debug" | "none" => OptLevel::Debug,
                    other => return Err(ProjectError::Parse(format!("Unknown optimize level: '{}'", other))),
                };
            }
            "dependencies" => {
                dependencies = parse_dependencies(value, base_dir)?;
            }
            "sandbox" => {
                sandbox = parse_sandbox(value)?;
            }
            _ => {
                // Unknown key — warn but don't fail
            }
        }
    }

    // Validate entry exists relative to base_dir
    let abs_entry = base_dir.join(&entry);
    if !abs_entry.exists() {
        return Err(ProjectError::Validation(format!(
            "Entry point '{}' not found (resolved to '{}')",
            entry.display(), abs_entry.display()
        )));
    }

    Ok(ProjectConfig {
        name,
        version,
        author,
        entry: abs_entry,
        output_type,
        optimize,
        dependencies,
        sandbox,
    })
}

// ─── Internal Helpers ────────────────────────────────────────────────────

fn extract_project_name(content: &str) -> Result<String, ProjectError> {
    // Match: project("Name") {
    for line in content.lines() {
        let t = line.trim();
        if t.starts_with("project(") || t.starts_with("project (") {
            if let Some(start) = t.find('"') {
                if let Some(end) = t[start + 1..].find('"') {
                    return Ok(t[start + 1..start + 1 + end].to_string());
                }
            }
        }
    }
    Err(ProjectError::Parse("Missing project(\"name\") declaration".into()))
}

fn extract_top_block(content: &str) -> Result<String, ProjectError> {
    // Find the first '{' after project(...) and grab everything to the matching '}'
    if let Some(open) = content.find('{') {
        let rest = &content[open + 1..];
        let mut depth = 1;
        let mut end = 0;
        for (i, ch) in rest.char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 { end = i; break; }
                }
                _ => {}
            }
        }
        if depth == 0 {
            return Ok(rest[..end].to_string());
        }
    }
    Err(ProjectError::Parse("Unmatched braces in .kicomp file".into()))
}

/// Parse simple `key: value` pairs from a block string.
/// Supports nested blocks (key: { ... }) returned as raw string values.
fn parse_block_fields(block: &str) -> Result<Vec<(String, String)>, ProjectError> {
    let mut results = Vec::new();
    let chars: Vec<char> = block.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        // Skip whitespace
        while i < len && chars[i].is_whitespace() { i += 1; }
        if i >= len { break; }

        // Read key (until ':')
        let key_start = i;
        while i < len && chars[i] != ':' && chars[i] != '{' && chars[i] != '}' { i += 1; }
        if i >= len { break; }
        if chars[i] != ':' { i += 1; continue; }

        let key = chars[key_start..i].iter().collect::<String>().trim().to_string();
        i += 1; // skip ':'

        // Skip whitespace after ':'
        while i < len && (chars[i] == ' ' || chars[i] == '\t') { i += 1; }

        // Determine value type
        if i < len && chars[i] == '{' {
            // Nested block — find matching '}'
            let block_start = i;
            let mut depth = 0;
            while i < len {
                if chars[i] == '{' { depth += 1; }
                if chars[i] == '}' { depth -= 1; if depth == 0 { i += 1; break; } }
                i += 1;
            }
            let value = chars[block_start..i].iter().collect::<String>();
            results.push((key, value));
        } else {
            // Simple value — read to end of line or comma
            let val_start = i;
            while i < len && chars[i] != '\n' && chars[i] != ',' { i += 1; }
            let value = chars[val_start..i].iter().collect::<String>().trim().to_string();
            results.push((key, value));
            if i < len && chars[i] == ',' { i += 1; }
        }
    }

    Ok(results)
}

fn unquote(s: &str) -> Result<String, ProjectError> {
    let t = s.trim().trim_matches(',');
    if t.starts_with('"') && t.ends_with('"') && t.len() >= 2 {
        Ok(t[1..t.len() - 1].to_string())
    } else {
        // Try returning as-is for unquoted values
        Ok(t.to_string())
    }
}

fn parse_dependencies(block: &str, base_dir: &Path) -> Result<Vec<Dependency>, ProjectError> {
    // Block is `{ "name": "path", ... }`
    let inner = extract_inner_block(block)?;
    let pairs = parse_block_fields(&inner)?;
    let mut deps = Vec::new();

    for (name, value) in pairs {
        let name = name.trim_matches('"').to_string();
        let path_str = unquote(&value)?;
        let dep_path = base_dir.join(&path_str);
        deps.push(Dependency {
            name,
            source: DependencySource::Local(dep_path),
        });
    }

    Ok(deps)
}

fn parse_sandbox(block: &str) -> Result<SandboxConfig, ProjectError> {
    let inner = extract_inner_block(block)?;
    let pairs = parse_block_fields(&inner)?;
    let mut config = SandboxConfig::default();

    for (key, value) in pairs {
        match key.as_str() {
            "allow_network" => config.allow_network = parse_bool(&value)?,
            "allow_fs_read" => config.allow_fs_read = parse_bool(&value)?,
            "allow_audio" => config.allow_audio = parse_bool(&value)?,
            "allow_exec" => config.allow_exec = parse_bool(&value)?,
            "allow_threads" => config.allow_threads = parse_bool(&value)?,
            "allow_fs_write" => {
                // Can be bool or array of paths
                let t = value.trim().trim_matches(',');
                if t == "true" {
                    config.allow_fs_write = vec!["*".to_string()]; // all paths
                } else if t == "false" {
                    config.allow_fs_write = Vec::new();
                } else if t.starts_with('[') {
                    // Parse array: ["./logs", "./data"]
                    let inner = t.trim_start_matches('[').trim_end_matches(']');
                    config.allow_fs_write = inner.split(',')
                        .map(|s| s.trim().trim_matches('"').to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                }
            }
            _ => {}
        }
    }

    Ok(config)
}

fn extract_inner_block(block: &str) -> Result<String, ProjectError> {
    let t = block.trim();
    if t.starts_with('{') && t.ends_with('}') {
        Ok(t[1..t.len() - 1].to_string())
    } else {
        Err(ProjectError::Parse(format!("Expected block {{...}}, got: {}", t)))
    }
}

fn parse_bool(s: &str) -> Result<bool, ProjectError> {
    match s.trim().trim_matches(',') {
        "true" => Ok(true),
        "false" => Ok(false),
        other => Err(ProjectError::Parse(format!("Expected true/false, got: '{}'", other))),
    }
}

/// Generate a scaffold `project.kicomp` file content for `kivm init`.
pub fn scaffold_kicomp(name: &str) -> String {
    format!(r#"// {name}.kicomp — Kinetix Project Configuration
project("{name}") {{
    version: "0.1.0"
    author: ""

    // Build target: "kivm" (bytecode) or "native" (LLVM AOT)
    output_type: "kivm"

    // Main entry point
    entry: "src/main.kix"

    // External dependencies
    dependencies: {{
    }}

    // Security permissions for the sandbox
    sandbox: {{
        allow_network: false,
        allow_fs_write: [],
        allow_audio: false,
        allow_exec: false,
        allow_threads: false
    }}

    // Optimization level: "debug", "speed", or "size"
    optimize: "debug"
}}
"#, name = name)
}

/// Generate a scaffold `src/main.kix` file content.
pub fn scaffold_main_kix(project_name: &str) -> String {
    format!(r#"// {project_name} — Kinetix Application
// Created with kivm init

println("Hello from {project_name}!")
"#, project_name = project_name)
}

// ─── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_project_name() {
        let content = r#"project("MyApp") {
            version: "1.0.0"
        }"#;
        let name = extract_project_name(content).unwrap();
        assert_eq!(name, "MyApp");
    }

    #[test]
    fn test_parse_simple_fields() {
        let block = r#"
            version: "1.0.0"
            entry: "src/main.kix"
            output_type: "kivm"
        "#;
        let fields = parse_block_fields(block).unwrap();
        assert_eq!(fields.len(), 3);
        assert_eq!(fields[0].0, "version");
    }

    #[test]
    fn test_sandbox_to_capabilities() {
        let sandbox = SandboxConfig {
            allow_network: true,
            allow_fs_read: true,
            allow_fs_write: vec!["./logs".into()],
            allow_audio: false,
            allow_exec: false,
            allow_threads: true,
        };
        let caps = sandbox.to_capabilities();
        assert!(caps.contains(&Capability::FsRead));
        assert!(caps.contains(&Capability::FsWrite));
        assert!(caps.contains(&Capability::NetAccess));
        assert!(caps.contains(&Capability::ThreadControl));
        assert!(!caps.contains(&Capability::OsExecute));
    }

    #[test]
    fn test_scaffold_valid() {
        let content = scaffold_kicomp("TestApp");
        assert!(content.contains("TestApp"));
        assert!(content.contains("src/main.kix"));
    }
}
