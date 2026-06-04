//! Repo Map — Code graph understanding for OpenShark
//!
//! Builds a lightweight structural map of the codebase for LLM context.
//! Inspired by Aider's repo map.

#![allow(dead_code)]

use anyhow::Result;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SymbolKind {
    Function,
    Struct,
    Enum,
    Trait,
    Impl,
    Module,
    Const,
    Static,
    Macro,
    Type,
    Unknown,
}

impl std::fmt::Display for SymbolKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SymbolKind::Function => write!(f, "fn"),
            SymbolKind::Struct => write!(f, "struct"),
            SymbolKind::Enum => write!(f, "enum"),
            SymbolKind::Trait => write!(f, "trait"),
            SymbolKind::Impl => write!(f, "impl"),
            SymbolKind::Module => write!(f, "mod"),
            SymbolKind::Const => write!(f, "const"),
            SymbolKind::Static => write!(f, "static"),
            SymbolKind::Macro => write!(f, "macro"),
            SymbolKind::Type => write!(f, "type"),
            SymbolKind::Unknown => write!(f, "?"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SymbolNode {
    pub name: String,
    pub kind: SymbolKind,
    pub file: String,
    pub line: usize,
    pub context: String, // surrounding line for context
}

#[derive(Debug, Clone)]
pub struct FileNode {
    pub path: String,
    pub language: String,
    pub lines: usize,
}

#[derive(Debug, Clone, Default)]
pub struct RepoMap {
    pub files: Vec<FileNode>,
    pub symbols: Vec<SymbolNode>,
    pub root: String,
}

impl RepoMap {
    pub fn find_symbol(&self, name: &str) -> Option<&SymbolNode> {
        self.symbols.iter().find(|s| s.name == name)
    }

    pub fn find_symbols_by_kind(&self, kind: SymbolKind) -> Vec<&SymbolNode> {
        self.symbols.iter().filter(|s| s.kind == kind).collect()
    }

    pub fn files_by_language(&self, lang: &str) -> Vec<&FileNode> {
        self.files.iter().filter(|f| f.language == lang).collect()
    }

    pub fn stats(&self) -> HashMap<String, usize> {
        let mut stats = HashMap::new();
        stats.insert("files".to_string(), self.files.len());
        stats.insert("symbols".to_string(), self.symbols.len());
        for file in &self.files {
            *stats.entry(format!("lang:{}", file.language)).or_insert(0) += 1;
        }
        for sym in &self.symbols {
            *stats.entry(format!("kind:{}", sym.kind)).or_insert(0) += 1;
        }
        stats
    }
}

fn detect_language(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("rs") => "rust",
        Some("py") | Some("pyi") => "python",
        Some("js") | Some("mjs") | Some("cjs") => "javascript",
        Some("ts") | Some("tsx") => "typescript",
        Some("go") => "go",
        Some("c") | Some("h") => "c",
        Some("cpp") | Some("cc") | Some("hpp") | Some("cxx") => "cpp",
        Some("java") => "java",
        Some("rb") => "ruby",
        Some("php") => "php",
        Some("swift") => "swift",
        Some("kt") => "kotlin",
        Some("scala") => "scala",
        Some("sh") | Some("bash") => "shell",
        Some("yaml") | Some("yml") => "yaml",
        Some("json") => "json",
        Some("toml") => "toml",
        Some("md") | Some("markdown") => "markdown",
        _ => "unknown",
    }
}

fn extract_symbols(content: &str, file_path: &str, language: &str) -> Vec<SymbolNode> {
    let mut symbols = Vec::new();

    let patterns: Vec<(SymbolKind, regex::Regex)> = match language {
        "rust" => vec![
            (SymbolKind::Function, regex::Regex::new(r"^\s*(?:pub\s+)?(?:async\s+)?fn\s+(\w+)").unwrap()),
            (SymbolKind::Struct, regex::Regex::new(r"^\s*(?:pub\s+)?struct\s+(\w+)").unwrap()),
            (SymbolKind::Enum, regex::Regex::new(r"^\s*(?:pub\s+)?enum\s+(\w+)").unwrap()),
            (SymbolKind::Trait, regex::Regex::new(r"^\s*(?:pub\s+)?trait\s+(\w+)").unwrap()),
            (SymbolKind::Impl, regex::Regex::new(r"^\s*impl\s+(?:<[^>]+>\s+)?(\w+)").unwrap()),
            (SymbolKind::Module, regex::Regex::new(r"^\s*(?:pub\s+)?mod\s+(\w+)").unwrap()),
            (SymbolKind::Const, regex::Regex::new(r"^\s*(?:pub\s+)?const\s+\w+:\s+[^=]+=\s+").unwrap()),
            (SymbolKind::Macro, regex::Regex::new(r"^\s*macro_rules!\s+(\w+)").unwrap()),
            (SymbolKind::Type, regex::Regex::new(r"^\s*(?:pub\s+)?type\s+(\w+)").unwrap()),
        ],
        "python" => vec![
            (SymbolKind::Function, regex::Regex::new(r"^\s*def\s+(\w+)").unwrap()),
            (SymbolKind::Struct, regex::Regex::new(r"^\s*class\s+(\w+)").unwrap()),
            (SymbolKind::Const, regex::Regex::new(r"^([A-Z_][A-Z0-9_]*)\s*=").unwrap()),
        ],
        "javascript" | "typescript" => vec![
            (SymbolKind::Function, regex::Regex::new(r"^\s*(?:export\s+)?(?:async\s+)?function\s+(\w+)").unwrap()),
            (SymbolKind::Function, regex::Regex::new(r"^\s*(?:export\s+)?const\s+(\w+)\s*=\s*(?:async\s+)?\(").unwrap()),
            (SymbolKind::Struct, regex::Regex::new(r"^\s*(?:export\s+)?(?:class|interface)\s+(\w+)").unwrap()),
            (SymbolKind::Const, regex::Regex::new(r"^\s*(?:export\s+)?const\s+(\w+)\s*=").unwrap()),
        ],
        "go" => vec![
            (SymbolKind::Trait, regex::Regex::new(r"^\s*func\s+(?:\([^)]+\)\s+)?(\w+)").unwrap()),
            (SymbolKind::Struct, regex::Regex::new(r"^\s*type\s+(\w+)\s+struct").unwrap()),
            (SymbolKind::Trait, regex::Regex::new(r"^\s*type\s+(\w+)\s+interface").unwrap()),
        ],
        "c" | "cpp" => vec![
            (SymbolKind::Function, regex::Regex::new(r"^\s*(?:[\w:*&<>]+\s+)+(\w+)\s*\([^)]*\)\s*(?:const\s*)?\{").unwrap()),
            (SymbolKind::Struct, regex::Regex::new(r"^\s*(?:typedef\s+)?struct\s+(\w+)").unwrap()),
            (SymbolKind::Enum, regex::Regex::new(r"^\s*(?:typedef\s+)?enum\s+(\w+)").unwrap()),
        ],
        _ => vec![],
    };

    for (line_num, line) in content.lines().enumerate() {
        for (kind, re) in &patterns {
            if let Some(cap) = re.captures(line) {
                if let Some(name_match) = cap.get(1) {
                    symbols.push(SymbolNode {
                        name: name_match.as_str().to_string(),
                        kind: kind.clone(),
                        file: file_path.to_string(),
                        line: line_num + 1,
                        context: line.trim().to_string(),
                    });
                }
            }
        }
    }

    symbols
}

pub fn build_repo_map(root: &str) -> Result<RepoMap> {
    let mut map = RepoMap {
        root: root.to_string(),
        ..Default::default()
    };

    let ignore_dirs: std::collections::HashSet<&str> = [
        "target", "node_modules", ".git", "dist", "build", "out",
        ".venv", "venv", "__pycache__", ".pytest_cache", ".cargo",
    ].iter().copied().collect();

    for entry in walkdir::WalkDir::new(root)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            !name.starts_with('.') || name == "."
        })
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        // Skip ignored dirs
        if path.components().any(|c| {
            if let Some(name) = c.as_os_str().to_str() {
                ignore_dirs.contains(name)
            } else {
                false
            }
        }) {
            continue;
        }

        let rel_path = path.strip_prefix(root).unwrap_or(path).to_string_lossy().to_string();
        let language = detect_language(path).to_string();

        let content = std::fs::read_to_string(path).unwrap_or_default();
        let lines = content.lines().count();

        map.files.push(FileNode {
            path: rel_path.clone(),
            language: language.clone(),
            lines,
        });

        let symbols = extract_symbols(&content, &rel_path, &language);
        map.symbols.extend(symbols);
    }

    Ok(map)
}

pub fn format_repo_map(map: &RepoMap) -> String {
    let mut lines = vec![
        format!("📁 Repo Map: {} ({} files, {} symbols)", map.root, map.files.len(), map.symbols.len()),
        "═".repeat(60),
    ];

    // Language breakdown
    let mut lang_counts: HashMap<&str, usize> = HashMap::new();
    for f in &map.files {
        *lang_counts.entry(f.language.as_str()).or_insert(0) += 1;
    }
    let mut lang_vec: Vec<_> = lang_counts.into_iter().collect();
    lang_vec.sort_by(|a, b| b.1.cmp(&a.1));

    lines.push("\n📊 Languages:".to_string());
    for (lang, count) in lang_vec {
        lines.push(format!("  {:12} {} files", lang, count));
    }

    // Key symbols (limit to avoid token bloat)
    lines.push("\n🔣 Key Symbols:".to_string());
    for sym in map.symbols.iter().take(100) {
        lines.push(format!(
            "  {:10} {:20} → {}:{}",
            sym.kind.to_string(),
            sym.name,
            sym.file,
            sym.line
        ));
    }
    if map.symbols.len() > 100 {
        lines.push(format!("  ... and {} more symbols", map.symbols.len() - 100));
    }

    lines.join("\n")
}

pub fn format_repo_map_compact(map: &RepoMap) -> String {
    let mut lines = vec![format!("Repo: {} | Files: {} | Symbols: {}", map.root, map.files.len(), map.symbols.len())];

    for sym in map.symbols.iter().take(50) {
        lines.push(format!("{} {} ({}:{})", sym.kind, sym.name, sym.file, sym.line));
    }
    if map.symbols.len() > 50 {
        lines.push(format!("... {} more", map.symbols.len() - 50));
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    static CLEANUP_LOCK: Mutex<()> = Mutex::new(());

    fn temp_rust_project() -> String {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = format!("/tmp/openshark_repo_test_{}_{n}", std::process::id());
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(format!("{}/src", dir)).unwrap();
        fs::write(format!("{}/src/main.rs", dir), r#"
fn main() {
    println!("hello");
}

pub struct MyStruct {
    value: i32,
}

impl MyStruct {
    pub fn new() -> Self {
        Self { value: 0 }
    }
}

enum Status {
    Ok,
    Err,
}
"#).unwrap();
        fs::write(format!("{}/Cargo.toml", dir), r#"
[package]
name = "test"
version = "0.1.0"
"#).unwrap();
        dir
    }

    fn cleanup(dir: &str) {
        let _guard = CLEANUP_LOCK.lock().unwrap();
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_detect_language() {
        assert_eq!(detect_language(Path::new("foo.rs")), "rust");
        assert_eq!(detect_language(Path::new("foo.py")), "python");
        assert_eq!(detect_language(Path::new("foo.ts")), "typescript");
        assert_eq!(detect_language(Path::new("foo.go")), "go");
        assert_eq!(detect_language(Path::new("foo.c")), "c");
        assert_eq!(detect_language(Path::new("foo.cpp")), "cpp");
    }

    #[test]
    fn test_build_repo_map() {
        let dir = temp_rust_project();
        let map = build_repo_map(&dir).unwrap();

        assert!(!map.files.is_empty());
        assert!(map.find_symbol("main").is_some());
        assert!(map.find_symbol("MyStruct").is_some());
        assert!(map.find_symbol("Status").is_some());

        let fns = map.find_symbols_by_kind(SymbolKind::Function);
        assert!(!fns.is_empty());

        cleanup(&dir);
    }

    #[test]
    fn test_format_repo_map() {
        let dir = temp_rust_project();
        let map = build_repo_map(&dir).unwrap();
        let formatted = format_repo_map(&map);
        assert!(formatted.contains("main"));
        assert!(formatted.contains("MyStruct"));
        cleanup(&dir);
    }
}
