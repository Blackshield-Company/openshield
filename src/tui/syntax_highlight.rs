use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

/// Simple syntax highlighter for code blocks.
/// Supports Rust, Python, JavaScript, TypeScript, JSON, TOML, YAML, Bash, and generic code.
pub fn highlight_code_block(code: &str, lang: &str) -> Vec<Line<'static>> {
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

// ── Rust ───────────────────────────────────────────────────────────────────
fn highlight_rust(lines: &[&str]) -> Vec<Line<'static>> {
    let keywords = [
        "fn", "let", "mut", "const", "static", "struct", "enum", "trait", "impl", "pub", "use",
        "mod", "match", "if", "else", "for", "while", "loop", "return", "break", "continue",
        "async", "await", "move", "where", "type", "as", "ref", "self", "Self", "super", "crate",
        "unsafe", "extern", "dyn", "box", "yield", "try", "macro",
    ];
    let types = [
        "i8",
        "i16",
        "i32",
        "i64",
        "i128",
        "isize",
        "u8",
        "u16",
        "u32",
        "u64",
        "u128",
        "usize",
        "f32",
        "f64",
        "bool",
        "char",
        "str",
        "String",
        "Vec",
        "Option",
        "Result",
        "HashMap",
        "BTreeMap",
        "Arc",
        "Rc",
        "Box",
        "Pin",
        "Cell",
        "RefCell",
        "VecDeque",
        "HashSet",
        "BTreeSet",
        "LinkedList",
    ];
    let builtins = [
        "println!",
        "print!",
        "format!",
        "vec!",
        "assert!",
        "assert_eq!",
        "panic!",
        "todo!",
        "unimplemented!",
        "unwrap",
        "expect",
        "clone",
        "len",
        "push",
        "pop",
        "insert",
        "remove",
        "get",
        "iter",
        "collect",
        "map",
        "filter",
        "fold",
        "zip",
        "enumerate",
        "chars",
        "lines",
        "to_string",
        "parse",
        "into",
        "from",
        "default",
        "new",
        "with_capacity",
    ];

    lines
        .iter()
        .map(|line| {
            let spans = tokenize_and_highlight(line, &keywords, &types, &builtins);
            Line::from(spans)
        })
        .collect()
}

// ── Python ─────────────────────────────────────────────────────────────────
fn highlight_python(lines: &[&str]) -> Vec<Line<'static>> {
    let keywords = [
        "def", "class", "if", "elif", "else", "for", "while", "try", "except", "finally", "with",
        "as", "import", "from", "return", "yield", "async", "await", "lambda", "pass", "break",
        "continue", "raise", "assert", "del", "global", "nonlocal", "in", "is", "not", "and", "or",
        "True", "False", "None",
    ];
    let types = [
        "int",
        "float",
        "str",
        "bool",
        "list",
        "dict",
        "tuple",
        "set",
        "frozenset",
        "bytes",
        "bytearray",
        "memoryview",
        "object",
    ];
    let builtins = [
        "print",
        "len",
        "range",
        "enumerate",
        "zip",
        "map",
        "filter",
        "sum",
        "min",
        "max",
        "sorted",
        "reversed",
        "open",
        "input",
        "isinstance",
        "hasattr",
        "getattr",
        "setattr",
        "delattr",
        "type",
        "id",
        "repr",
        "str",
        "int",
        "float",
        "list",
        "dict",
        "append",
        "extend",
        "insert",
        "remove",
        "pop",
        "clear",
        "keys",
        "values",
        "items",
        "get",
        "update",
        "join",
        "split",
    ];

    lines
        .iter()
        .map(|line| {
            let spans = tokenize_and_highlight(line, &keywords, &types, &builtins);
            Line::from(spans)
        })
        .collect()
}

// ── JavaScript / TypeScript ────────────────────────────────────────────────
fn highlight_js(lines: &[&str]) -> Vec<Line<'static>> {
    let keywords = [
        "function",
        "const",
        "let",
        "var",
        "if",
        "else",
        "for",
        "while",
        "do",
        "switch",
        "case",
        "break",
        "continue",
        "return",
        "try",
        "catch",
        "finally",
        "throw",
        "new",
        "this",
        "typeof",
        "instanceof",
        "void",
        "delete",
        "in",
        "of",
        "await",
        "async",
        "yield",
        "class",
        "extends",
        "super",
        "import",
        "export",
        "from",
        "default",
        "interface",
        "type",
        "enum",
        "namespace",
        "module",
        "declare",
        "public",
        "private",
        "protected",
        "readonly",
        "abstract",
        "implements",
    ];
    let types = [
        "string",
        "number",
        "boolean",
        "symbol",
        "bigint",
        "undefined",
        "null",
        "any",
        "unknown",
        "never",
        "void",
        "object",
        "Array",
        "Promise",
        "Map",
        "Set",
        "Date",
        "RegExp",
        "Error",
        "Function",
    ];
    let builtins = [
        "console",
        "log",
        "warn",
        "error",
        "info",
        "JSON",
        "parse",
        "stringify",
        "Math",
        "random",
        "floor",
        "ceil",
        "round",
        "abs",
        "min",
        "max",
        "setTimeout",
        "setInterval",
        "clearTimeout",
        "clearInterval",
        "fetch",
        "then",
        "catch",
        "finally",
        "push",
        "pop",
        "shift",
        "unshift",
        "slice",
        "splice",
        "concat",
        "join",
        "split",
        "map",
        "filter",
        "reduce",
        "forEach",
        "find",
        "includes",
        "indexOf",
        "toString",
        "valueOf",
        "hasOwnProperty",
    ];

    lines
        .iter()
        .map(|line| {
            let spans = tokenize_and_highlight(line, &keywords, &types, &builtins);
            Line::from(spans)
        })
        .collect()
}

// ── JSON ───────────────────────────────────────────────────────────────────
fn highlight_json(lines: &[&str]) -> Vec<Line<'static>> {
    lines
        .iter()
        .map(|line| {
            let mut spans = Vec::new();
            let mut chars = line.chars().peekable();

            while let Some(ch) = chars.next() {
                match ch {
                    '{' | '}' | '[' | ']' | ':' | ',' => {
                        spans.push(Span::styled(
                            ch.to_string(),
                            Style::default().fg(Color::White),
                        ));
                    }
                    '"' => {
                        let mut string = String::from('"');
                        for c in chars.by_ref() {
                            string.push(c);
                            if c == '"' {
                                break;
                            }
                        }
                        // Keys vs values: key is before colon
                        let is_key = line.trim().starts_with(&string)
                            || line[..line.find(&string).unwrap_or(0)]
                                .trim_end()
                                .ends_with(',');
                        let color = if is_key { Color::Cyan } else { Color::Green };
                        spans.push(Span::styled(string, Style::default().fg(color)));
                    }
                    't' if line.trim().starts_with("true")
                        || line[line.find('t').unwrap_or(0)..].starts_with("true") =>
                    {
                        spans.push(Span::styled(
                            "true".to_string(),
                            Style::default().fg(Color::Magenta),
                        ));
                        for _ in 0..3 {
                            chars.next();
                        }
                    }
                    'f' if line.trim().starts_with("false")
                        || line[line.find('f').unwrap_or(0)..].starts_with("false") =>
                    {
                        spans.push(Span::styled(
                            "false".to_string(),
                            Style::default().fg(Color::Magenta),
                        ));
                        for _ in 0..4 {
                            chars.next();
                        }
                    }
                    'n' if line.trim().starts_with("null")
                        || line[line.find('n').unwrap_or(0)..].starts_with("null") =>
                    {
                        spans.push(Span::styled(
                            "null".to_string(),
                            Style::default().fg(Color::Magenta),
                        ));
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
                        spans.push(Span::styled(num, Style::default().fg(Color::Yellow)));
                    }
                    c => {
                        spans.push(Span::styled(
                            c.to_string(),
                            Style::default().fg(Color::Gray),
                        ));
                    }
                }
            }
            Line::from(spans)
        })
        .collect()
}

// ── TOML ───────────────────────────────────────────────────────────────────
fn highlight_toml(lines: &[&str]) -> Vec<Line<'static>> {
    lines
        .iter()
        .map(|line| {
            let mut spans = Vec::new();
            let trimmed = line.trim();

            if trimmed.starts_with('#') {
                spans.push(Span::styled(
                    line.to_string(),
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::ITALIC),
                ));
            } else if trimmed.starts_with('[') && trimmed.ends_with(']') {
                spans.push(Span::styled(
                    line.to_string(),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ));
            } else if let Some(pos) = line.find('=') {
                let key = &line[..pos];
                let rest = &line[pos..];
                spans.push(Span::styled(
                    key.to_string(),
                    Style::default().fg(Color::Cyan),
                ));
                spans.push(Span::styled(
                    rest.to_string(),
                    Style::default().fg(Color::White),
                ));
            } else {
                spans.push(Span::styled(
                    line.to_string(),
                    Style::default().fg(Color::White),
                ));
            }
            Line::from(spans)
        })
        .collect()
}

// ── YAML ───────────────────────────────────────────────────────────────────
fn highlight_yaml(lines: &[&str]) -> Vec<Line<'static>> {
    lines
        .iter()
        .map(|line| {
            let mut spans = Vec::new();
            let trimmed = line.trim();

            if trimmed.starts_with('#') {
                spans.push(Span::styled(
                    line.to_string(),
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::ITALIC),
                ));
            } else if trimmed.ends_with(':') {
                spans.push(Span::styled(
                    line.to_string(),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ));
            } else if trimmed == "-" || trimmed.starts_with("- ") {
                spans.push(Span::styled(
                    line.to_string(),
                    Style::default().fg(Color::Yellow),
                ));
            } else if trimmed == "true" || trimmed == "false" || trimmed == "null" || trimmed == "~"
            {
                spans.push(Span::styled(
                    line.to_string(),
                    Style::default().fg(Color::Magenta),
                ));
            } else {
                spans.push(Span::styled(
                    line.to_string(),
                    Style::default().fg(Color::White),
                ));
            }
            Line::from(spans)
        })
        .collect()
}

// ── Bash ───────────────────────────────────────────────────────────────────
fn highlight_bash(lines: &[&str]) -> Vec<Line<'static>> {
    let keywords = [
        "if", "then", "else", "elif", "fi", "for", "while", "do", "done", "case", "esac", "in",
        "function", "return", "exit", "break", "continue", "local", "export", "unset", "readonly",
        "shift", "source", ".",
    ];
    let builtins = [
        "echo", "printf", "cat", "grep", "sed", "awk", "cut", "sort", "uniq", "head", "tail", "wc",
        "find", "xargs", "chmod", "chown", "mkdir", "rm", "cp", "mv", "ln", "touch", "ls", "cd",
        "pwd", "which", "curl", "wget", "git", "docker", "cargo", "npm", "yarn", "node", "python",
        "python3", "pip", "rustc", "make", "cmake", "tar", "zip",
    ];

    lines
        .iter()
        .map(|line| {
            let spans = tokenize_and_highlight(line, &keywords, &[], &builtins);
            Line::from(spans)
        })
        .collect()
}

// ── Generic ────────────────────────────────────────────────────────────────
fn highlight_generic(lines: &[&str]) -> Vec<Line<'static>> {
    lines
        .iter()
        .map(|line| {
            Line::from(vec![Span::styled(
                line.to_string(),
                Style::default().fg(Color::White),
            )])
        })
        .collect()
}

// ── Shared tokenizer ───────────────────────────────────────────────────────
fn tokenize_and_highlight(
    line: &str,
    keywords: &[&str],
    types: &[&str],
    builtins: &[&str],
) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut chars = line.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            // Comments
            '/' if chars.peek() == Some(&'/') => {
                let mut comment = String::from("//");
                for c in chars.by_ref() {
                    comment.push(c);
                }
                spans.push(Span::styled(
                    comment,
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::ITALIC),
                ));
                break;
            }
            '#' => {
                let mut comment = String::from('#');
                for c in chars.by_ref() {
                    comment.push(c);
                }
                spans.push(Span::styled(
                    comment,
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::ITALIC),
                ));
                break;
            }
            // Strings
            '"' => {
                let mut string = String::from('"');
                for c in chars.by_ref() {
                    string.push(c);
                    if c == '"' && !string.ends_with("\\\"") {
                        break;
                    }
                }
                spans.push(Span::styled(string, Style::default().fg(Color::Green)));
            }
            '\'' => {
                let mut string = String::from('\'');
                for c in chars.by_ref() {
                    string.push(c);
                    if c == '\'' {
                        break;
                    }
                }
                spans.push(Span::styled(string, Style::default().fg(Color::Green)));
            }
            // Raw strings (Rust r#"..."#)
            'r' if chars.peek() == Some(&'#') || chars.peek() == Some(&'"') => {
                let mut raw = String::from('r');
                let mut hash_count = 0;
                while let Some(&'#') = chars.peek() {
                    raw.push(chars.next().expect("peek confirmed element exists"));
                    hash_count += 1;
                }
                if chars.peek() == Some(&'"') {
                    raw.push(chars.next().expect("peek confirmed element exists"));
                    // Read until closing "#
                    while let Some(c) = chars.next() {
                        raw.push(c);
                        if c == '"' {
                            let mut close_hashes = 0;
                            while close_hashes < hash_count {
                                if chars.peek() == Some(&'#') {
                                    raw.push(chars.next().expect("peek confirmed element exists"));
                                    close_hashes += 1;
                                } else {
                                    break;
                                }
                            }
                            if close_hashes == hash_count {
                                break;
                            }
                        }
                    }
                }
                spans.push(Span::styled(raw, Style::default().fg(Color::Green)));
            }
            // Numbers
            c if c.is_numeric() => {
                let mut num = String::from(c);
                while let Some(&next) = chars.peek() {
                    if next.is_numeric()
                        || next == '.'
                        || next == '_'
                        || next == 'x'
                        || next == 'b'
                        || next == 'o'
                        || next == 'e'
                        || next == 'E'
                    {
                        num.push(chars.next().expect("peek confirmed element exists"));
                    } else if next == 'u'
                        || next == 'i'
                        || next == 'f'
                        || next == 'U'
                        || next == 'I'
                        || next == 'F'
                    {
                        num.push(chars.next().expect("peek confirmed element exists"));
                        while let Some(&n) = chars.peek() {
                            if n.is_numeric() {
                                num.push(chars.next().expect("peek confirmed element exists"));
                            } else {
                                break;
                            }
                        }
                        break;
                    } else {
                        break;
                    }
                }
                spans.push(Span::styled(num, Style::default().fg(Color::Yellow)));
            }
            // Identifiers / keywords
            c if c.is_alphabetic() || c == '_' => {
                let mut ident = String::from(c);
                while let Some(&next) = chars.peek() {
                    if next.is_alphanumeric() || next == '_' || next == '!' {
                        ident.push(chars.next().expect("peek confirmed element exists"));
                    } else {
                        break;
                    }
                }
                let style = if keywords.contains(&ident.as_str()) {
                    Style::default()
                        .fg(Color::Magenta)
                        .add_modifier(Modifier::BOLD)
                } else if types.contains(&ident.as_str()) {
                    Style::default().fg(Color::Cyan)
                } else if builtins.contains(&ident.as_str()) {
                    Style::default().fg(Color::Blue)
                } else {
                    Style::default().fg(Color::White)
                };
                spans.push(Span::styled(ident, style));
            }
            // Macros (Rust)
            '!' if !spans.is_empty()
                && spans
                    .last()
                    .map(|s| {
                        let text = s.content.as_ref();
                        text.chars().all(|c| c.is_alphanumeric() || c == '_')
                    })
                    .unwrap_or(false) =>
            {
                spans.push(Span::styled(
                    "!".to_string(),
                    Style::default().fg(Color::Yellow),
                ));
            }
            // Whitespace and punctuation
            c => {
                spans.push(Span::styled(
                    c.to_string(),
                    Style::default().fg(Color::White),
                ));
            }
        }
    }

    spans
}

/// Detect code blocks in text and return highlighted lines interspersed with plain text.
/// Returns Vec of (is_code, lines) tuples.
pub fn extract_and_highlight(text: &str) -> Vec<(bool, Vec<Line<'static>>)> {
    let mut result = Vec::new();
    let mut in_code = false;
    let mut code_lang = String::new();
    let mut code_buffer = String::new();
    let mut plain_buffer = String::new();

    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            if in_code {
                // End code block
                let highlighted = highlight_code_block(&code_buffer, &code_lang);
                result.push((true, highlighted));
                code_buffer.clear();
                code_lang.clear();
                in_code = false;
            } else {
                // Start code block — flush plain text first
                if !plain_buffer.is_empty() {
                    let plain_lines: Vec<Line<'static>> = plain_buffer
                        .lines()
                        .map(|l| {
                            Line::from(vec![Span::styled(
                                l.to_string(),
                                Style::default().fg(Color::White),
                            )])
                        })
                        .collect();
                    result.push((false, plain_lines));
                    plain_buffer.clear();
                }
                code_lang = line.trim_start()[3..].trim().to_string();
                in_code = true;
            }
        } else if in_code {
            code_buffer.push_str(line);
            code_buffer.push('\n');
        } else {
            plain_buffer.push_str(line);
            plain_buffer.push('\n');
        }
    }

    // Flush remaining
    if !plain_buffer.is_empty() {
        let plain_lines: Vec<Line<'static>> = plain_buffer
            .lines()
            .map(|l| {
                Line::from(vec![Span::styled(
                    l.to_string(),
                    Style::default().fg(Color::White),
                )])
            })
            .collect();
        result.push((false, plain_lines));
    }
    if !code_buffer.is_empty() {
        let highlighted = highlight_code_block(&code_buffer, &code_lang);
        result.push((true, highlighted));
    }

    result
}

/// Wrap bare URLs in OSC 8 hyperlink escape sequences.
/// Format: \x1b]8;;URL\x07TEXT\x1b]8;;\x07
pub fn hyperlink_urls(text: &str) -> String {
    // Simple regex-like scan for http:// and https:// URLs
    let mut result = String::new();
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == 'h' {
            let mut potential = String::from('h');
            while let Some(&c) = chars.peek() {
                if c.is_alphanumeric()
                    || c == ':'
                    || c == '/'
                    || c == '.'
                    || c == '-'
                    || c == '_'
                    || c == '?'
                    || c == '&'
                    || c == '='
                    || c == '%'
                    || c == '#'
                    || c == '@'
                    || c == '+'
                    || c == '~'
                    || c == '['
                    || c == ']'
                    || c == '!'
                    || c == '$'
                    || c == '\''
                    || c == '('
                    || c == ')'
                    || c == '*'
                    || c == ','
                    || c == ';'
                {
                    potential.push(c);
                    chars.next();
                } else {
                    break;
                }
            }
            if potential.starts_with("http://") || potential.starts_with("https://") {
                // OSC 8 hyperlink
                result.push_str("\x1b]8;;");
                result.push_str(&potential);
                result.push('\x07');
                result.push_str(&potential);
                result.push_str("\x1b]8;;\x07");
            } else {
                result.push_str(&potential);
            }
        } else {
            result.push(ch);
        }
    }

    result
}

/// Render inline markdown within a plain text line, with OSC 8 hyperlink support.
/// Supports: **bold**, *italic*, `code`, [links](url), ~~strikethrough~~, bare URLs
pub fn render_markdown_line(line: &str) -> Line<'static> {
    // First, wrap bare URLs in OSC 8 hyperlink sequences
    let line = hyperlink_urls(line);
    let mut spans = Vec::new();
    let mut chars = line.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            // Bold: **text**
            '*' if chars.peek() == Some(&'*') => {
                chars.next(); // consume second *
                let mut text = String::new();
                while let Some(c) = chars.next() {
                    if c == '*' && chars.peek() == Some(&'*') {
                        chars.next();
                        break;
                    }
                    text.push(c);
                }
                spans.push(Span::styled(
                    text,
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ));
            }
            // Italic: *text* or _text_
            '*' | '_' => {
                let marker = ch;
                let mut text = String::new();
                for c in chars.by_ref() {
                    if c == marker {
                        break;
                    }
                    text.push(c);
                }
                spans.push(Span::styled(
                    text,
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::ITALIC),
                ));
            }
            // Inline code: `text`
            '`' => {
                let mut text = String::new();
                for c in chars.by_ref() {
                    if c == '`' {
                        break;
                    }
                    text.push(c);
                }
                spans.push(Span::styled(
                    text,
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ));
            }
            // Strikethrough: ~~text~~
            '~' if chars.peek() == Some(&'~') => {
                chars.next();
                let mut text = String::new();
                while let Some(c) = chars.next() {
                    if c == '~' && chars.peek() == Some(&'~') {
                        chars.next();
                        break;
                    }
                    text.push(c);
                }
                spans.push(Span::styled(
                    text,
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::CROSSED_OUT),
                ));
            }
            // Link: [text](url)
            '[' => {
                let mut text = String::new();
                for c in chars.by_ref() {
                    if c == ']' {
                        break;
                    }
                    text.push(c);
                }
                // Skip (url)
                if chars.peek() == Some(&'(') {
                    chars.next();
                    for c in chars.by_ref() {
                        if c == ')' {
                            break;
                        }
                    }
                }
                spans.push(Span::styled(
                    text,
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::UNDERLINED),
                ));
            }
            c => {
                // Accumulate plain text
                let mut plain = String::from(c);
                while let Some(&next) = chars.peek() {
                    if next == '*' || next == '_' || next == '`' || next == '~' || next == '[' {
                        break;
                    }
                    plain.push(chars.next().expect("peek confirmed element exists"));
                }
                spans.push(Span::styled(plain, Style::default().fg(Color::White)));
            }
        }
    }

    Line::from(spans)
}
