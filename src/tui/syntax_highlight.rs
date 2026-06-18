#![allow(dead_code)]

/// Simple syntax highlighter for code blocks.
/// Supports Rust, Python, JavaScript, TypeScript, JSON, TOML, YAML, Bash, and generic code.
/// Returns ANSI-colored strings instead of ratatui Span/Line types.
use crossterm::style::Color;
use crate::tui::theme::{ansi_fg, ansi_reset};

pub fn highlight_code_block(code: &str, lang: &str) -> Vec<String> {
    let lang_lower = lang.to_lowercase();
    let lines: Vec<&str> = code.lines().collect();

    match lang_lower.as_str() {
        "rust" | "rs" => highlight_rust(&lines),
        "python" | "py" => highlight_python(&lines),
        "javascript" | "js" | "typescript" | "ts" => highlight_js(&lines),
        "json" => highlight_json(&lines),
        "toml" => highlight_toml(&lines),
        "yaml" | "yml" => highlight_yaml(&lines),
        "bash" | "sh" | "shell" | "zsh" => highlight_bash(&lines),
        _ => highlight_generic(&lines),
    }
}

fn color_span(text: &str, color: Color) -> String {
    format!("{}{}{}", ansi_fg(color), text, ansi_reset())
}

fn bold_span(text: &str, color: Color) -> String {
    format!("\x1b[1m{}{}{}", ansi_fg(color), text, ansi_reset())
}

fn italic_span(text: &str, color: Color) -> String {
    format!("\x1b[3m{}{}{}", ansi_fg(color), text, ansi_reset())
}

// ── Rust ───────────────────────────────────────────────────────────────────
fn highlight_rust(lines: &[&str]) -> Vec<String> {
    let keywords = [
        "fn", "let", "mut", "const", "static", "struct", "enum", "trait", "impl", "pub", "use",
        "mod", "match", "if", "else", "for", "while", "loop", "return", "break", "continue",
        "async", "await", "move", "where", "type", "as", "ref", "self", "Self", "super", "crate",
        "unsafe", "extern", "dyn", "box", "yield", "try", "macro",
    ];
    let types = [
        "i8", "i16", "i32", "i64", "i128", "isize", "u8", "u16", "u32", "u64", "u128", "usize",
        "f32", "f64", "bool", "char", "str", "String", "Vec", "Option", "Result", "HashMap",
        "BTreeMap", "Arc", "Rc", "Box", "Pin", "Cell", "RefCell", "VecDeque", "HashSet",
        "BTreeSet", "LinkedList",
    ];
    let builtins = [
        "println!", "print!", "format!", "vec!", "assert!", "assert_eq!", "panic!", "todo!",
        "unimplemented!", "unwrap", "expect", "clone", "len", "push", "pop", "insert", "remove",
        "get", "iter", "collect", "map", "filter", "fold", "zip", "enumerate", "chars", "lines",
        "to_string", "parse", "into", "from", "default", "new", "with_capacity",
    ];

    lines
        .iter()
        .map(|line| tokenize_and_highlight(line, &keywords, &types, &builtins))
        .collect()
}

// ── Python ─────────────────────────────────────────────────────────────────
fn highlight_python(lines: &[&str]) -> Vec<String> {
    let keywords = [
        "def", "class", "if", "elif", "else", "for", "while", "try", "except", "finally", "with",
        "as", "import", "from", "return", "yield", "async", "await", "lambda", "pass", "break",
        "continue", "raise", "assert", "del", "global", "nonlocal", "in", "is", "not", "and", "or",
        "True", "False", "None",
    ];
    let types = [
        "int", "float", "str", "bool", "list", "dict", "tuple", "set", "frozenset", "bytes",
        "bytearray", "memoryview", "object",
    ];
    let builtins = [
        "print", "len", "range", "enumerate", "zip", "map", "filter", "sum", "min", "max",
        "sorted", "reversed", "open", "input", "isinstance", "hasattr", "getattr", "setattr",
        "delattr", "type", "id", "repr", "str", "int", "float", "list", "dict", "append",
        "extend", "insert", "remove", "pop", "clear", "keys", "values", "items", "get",
        "update", "join", "split",
    ];

    lines
        .iter()
        .map(|line| tokenize_and_highlight(line, &keywords, &types, &builtins))
        .collect()
}

// ── JavaScript / TypeScript ────────────────────────────────────────────────
fn highlight_js(lines: &[&str]) -> Vec<String> {
    let keywords = [
        "function", "const", "let", "var", "if", "else", "for", "while", "do", "switch", "case",
        "break", "continue", "return", "try", "catch", "finally", "throw", "new", "this", "typeof",
        "instanceof", "void", "delete", "in", "of", "await", "async", "yield", "class", "extends",
        "super", "import", "export", "from", "default", "interface", "type", "enum", "namespace",
        "module", "declare", "public", "private", "protected", "readonly", "abstract", "implements",
    ];
    let types = [
        "string", "number", "boolean", "symbol", "bigint", "undefined", "null", "any", "unknown",
        "never", "void", "object", "Array", "Promise", "Map", "Set", "Date", "RegExp", "Error",
        "Function",
    ];
    let builtins = [
        "console", "log", "warn", "error", "info", "JSON", "parse", "stringify", "Math", "random",
        "floor", "ceil", "round", "abs", "min", "max", "setTimeout", "setInterval", "clearTimeout",
        "clearInterval", "fetch", "then", "catch", "finally", "push", "pop", "shift", "unshift",
        "slice", "splice", "concat", "join", "split", "map", "filter", "reduce", "forEach", "find",
        "includes", "indexOf", "toString", "valueOf", "hasOwnProperty",
    ];

    lines
        .iter()
        .map(|line| tokenize_and_highlight(line, &keywords, &types, &builtins))
        .collect()
}

// ── JSON ───────────────────────────────────────────────────────────────────
fn highlight_json(lines: &[&str]) -> Vec<String> {
    lines
        .iter()
        .map(|line| {
            let mut result = String::new();
            let mut chars = line.chars().peekable();

            while let Some(ch) = chars.next() {
                match ch {
                    '{' | '}' | '[' | ']' | ':' | ',' => {
                        result.push_str(&color_span(&ch.to_string(), Color::White));
                    }
                    '"' => {
                        let mut string = String::from('"');
                        for c in chars.by_ref() {
                            string.push(c);
                            if c == '"' {
                                break;
                            }
                        }
                        let is_key = line.trim().starts_with(&string)
                            || line[..line.find(&string).unwrap_or(0)]
                                .trim_end()
                                .ends_with(',');
                        let color = if is_key { Color::Cyan } else { Color::Green };
                        result.push_str(&color_span(&string, color));
                    }
                    't' if line.trim().starts_with("true")
                        || line[line.find('t').unwrap_or(0)..].starts_with("true") =>
                    {
                        result.push_str(&color_span("true", Color::Magenta));
                        for _ in 0..3 {
                            chars.next();
                        }
                    }
                    'f' if line.trim().starts_with("false")
                        || line[line.find('f').unwrap_or(0)..].starts_with("false") =>
                    {
                        result.push_str(&color_span("false", Color::Magenta));
                        for _ in 0..4 {
                            chars.next();
                        }
                    }
                    'n' if line.trim().starts_with("null")
                        || line[line.find('n').unwrap_or(0)..].starts_with("null") =>
                    {
                        result.push_str(&color_span("null", Color::Magenta));
                        for _ in 0..3 {
                            chars.next();
                        }
                    }
                    c if c.is_numeric() || c == '-' => {
                        let mut num = String::from(c);
                        while let Some(&next) = chars.peek() {
                            if next.is_numeric()
                                || next == '.'
                                || next == 'e'
                                || next == 'E'
                                || next == '-'
                                || next == '+'
                            {
                                num.push(chars.next().expect("peek confirmed element exists"));
                            } else {
                                break;
                            }
                        }
                        result.push_str(&color_span(&num, Color::Yellow));
                    }
                    c => {
                        result.push_str(&color_span(&c.to_string(), Color::Grey));
                    }
                }
            }
            result
        })
        .collect()
}

// ── TOML ───────────────────────────────────────────────────────────────────
fn highlight_toml(lines: &[&str]) -> Vec<String> {
    lines
        .iter()
        .map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with('#') {
                italic_span(line, Color::DarkGrey)
            } else if trimmed.starts_with('[') && trimmed.ends_with(']') {
                bold_span(line, Color::Cyan)
            } else if let Some(pos) = line.find('=') {
                let key = &line[..pos];
                let rest = &line[pos..];
                format!("{}{}", color_span(key, Color::Cyan), color_span(rest, Color::White))
            } else {
                color_span(line, Color::White)
            }
        })
        .collect()
}

// ── YAML ───────────────────────────────────────────────────────────────────
fn highlight_yaml(lines: &[&str]) -> Vec<String> {
    lines
        .iter()
        .map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with('#') {
                italic_span(line, Color::DarkGrey)
            } else if trimmed.ends_with(':') {
                bold_span(line, Color::Cyan)
            } else if trimmed == "-" || trimmed.starts_with("- ") {
                color_span(line, Color::Yellow)
            } else if trimmed == "true" || trimmed == "false" || trimmed == "null" || trimmed == "~"
            {
                color_span(line, Color::Magenta)
            } else {
                color_span(line, Color::White)
            }
        })
        .collect()
}

// ── Bash ───────────────────────────────────────────────────────────────────
fn highlight_bash(lines: &[&str]) -> Vec<String> {
    let keywords = [
        "if", "then", "else", "elif", "fi", "for", "while", "do", "done", "case", "esac", "in",
        "function", "return", "exit", "break", "continue", "shift", "source", ".", "export",
        "unset", "local", "readonly", "declare", "typeset", "trap", "wait", "bg", "fg", "jobs",
        "kill", "exec", "eval", "set", "unset", "env", "alias", "unalias", "test", "[", "[[",
    ];
    let builtins = [
        "echo", "printf", "cat", "grep", "sed", "awk", "cut", "sort", "uniq", "wc", "head",
        "tail", "find", "xargs", "chmod", "chown", "cp", "mv", "rm", "mkdir", "rmdir", "ls",
        "cd", "pwd", "touch", "ln", "tar", "gzip", "gunzip", "zip", "unzip", "curl", "wget",
        "ssh", "scp", "rsync", "git", "docker", "kubectl", "npm", "yarn", "cargo", "make",
        "python", "python3", "node", "ruby", "perl", "bash", "sh", "zsh",
    ];

    lines
        .iter()
        .map(|line| tokenize_and_highlight(line, &keywords, &[], &builtins))
        .collect()
}

// ── Generic ────────────────────────────────────────────────────────────────
fn highlight_generic(lines: &[&str]) -> Vec<String> {
    lines.iter().map(|line| color_span(line, Color::White)).collect()
}

// ── Tokenizer ──────────────────────────────────────────────────────────────
fn tokenize_and_highlight(line: &str, keywords: &[&str], types: &[&str], builtins: &[&str]) -> String {
    let mut result = String::new();
    let mut chars = line.chars().peekable();
    let mut in_string = false;
    let mut string_delim = '"';
    let mut string_buf = String::new();
    let in_comment = false;
    let mut in_number = false;
    let mut num_buf = String::new();

    while let Some(ch) = chars.next() {
        // Comments
        if !in_string && !in_comment && ch == '/'
            && let Some(&next) = chars.peek()
                && next == '/' {
                    result.push_str(&italic_span(line, Color::DarkGrey));
                    break;
                }

        // Strings
        if !in_comment && (ch == '"' || ch == '\'') {
            if in_string && ch == string_delim {
                in_string = false;
                string_buf.push(ch);
                result.push_str(&color_span(&string_buf, Color::Green));
                string_buf.clear();
                continue;
            } else if !in_string {
                in_string = true;
                string_delim = ch;
                string_buf.push(ch);
                continue;
            }
        }

        if in_string {
            string_buf.push(ch);
            continue;
        }

        // Numbers
        if !in_comment && (ch.is_numeric() || (ch == '-' && num_buf.is_empty()))
            && !in_number {
                in_number = true;
                num_buf.push(ch);
                continue;
            }

        if in_number {
            if ch.is_numeric() || ch == '.' || ch == 'e' || ch == 'E' || ch == '_' {
                num_buf.push(ch);
                continue;
            } else {
                result.push_str(&color_span(&num_buf, Color::Yellow));
                num_buf.clear();
                in_number = false;
            }
        }

        // Words / tokens
        if ch.is_alphanumeric() || ch == '_' || ch == '!' {
            let mut word = String::from(ch);
            while let Some(&next) = chars.peek() {
                if next.is_alphanumeric() || next == '_' || next == '!' {
                    word.push(chars.next().expect("peek confirmed"));
                } else {
                    break;
                }
            }

            let color = if keywords.contains(&word.as_str()) {
                Color::Magenta
            } else if types.contains(&word.as_str()) {
                Color::Cyan
            } else if builtins.contains(&word.as_str()) {
                Color::Yellow
            } else {
                Color::White
            };

            result.push_str(&color_span(&word, color));
        } else {
            // Punctuation / operators
            let color = match ch {
                '(' | ')' | '{' | '}' | '[' | ']' | ';' | ',' | '.' => Color::White,
                '+' | '-' | '*' | '/' | '%' | '=' | '!' | '&' | '|' | '<' | '>' | '^' | '~' | '?' | ':' => Color::Red,
                '#' => Color::DarkGrey,
                '@' => Color::Yellow,
                _ => Color::White,
            };
            result.push_str(&color_span(&ch.to_string(), color));
        }
    }

    // Flush remaining buffers
    if in_number && !num_buf.is_empty() {
        result.push_str(&color_span(&num_buf, Color::Yellow));
    }
    if in_string && !string_buf.is_empty() {
        result.push_str(&color_span(&string_buf, Color::Green));
    }

    result
}
