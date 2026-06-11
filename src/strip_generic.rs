//--+ src/strip_generic.rs

/// Strip comments for various languages.
pub fn strip_comments(code: &str, extension: &str) -> String {
    match extension {
        "toml" | "env" => strip_hash_comments(code),
        "yml" | "yaml" | "sh" => strip_hash_comments(code),
        "sql" => strip_sql_comments(code),
        "proto" => strip_c_style_comments(code),
        "json" => code.to_string(),     // JSON has no comments
        _ => strip_hash_comments(code), // safe default
    }
}

/// Collapse consecutive blank lines into one, trim trailing whitespace.
pub fn collapse_whitespace(code: &str) -> String {
    let mut prev_blank = false;
    let mut result = Vec::new();

    for line in code.lines() {
        let is_blank = line.trim().is_empty();
        if is_blank && prev_blank {
            continue;
        }
        result.push(line.trim_end());
        prev_blank = is_blank;
    }

    result.join("\n")
}

fn strip_hash_comments(code: &str) -> String {
    let mut result = Vec::new();
    for line in code.lines() {
        let mut cleaned_line = String::new();
        let mut in_single_quote = false;
        let mut in_double_quote = false;
        let mut chars = line.chars().peekable();

        while let Some(c) = chars.next() {
            // If we hit a # outside of quotes, it's a comment
            if c == '#' && !in_single_quote && !in_double_quote {
                break; // Ignore the rest of the line
            }

            // Track quote state so we don't strip # inside strings
            if c == '"' && !in_single_quote {
                in_double_quote = !in_double_quote;
            }
            if c == '\'' && !in_double_quote {
                in_single_quote = !in_single_quote;
            }

            // Handle escaping in double quotes (e.g. "Hello \" world")
            if c == '\\' && in_double_quote {
                cleaned_line.push(c);
                if let Some(next_c) = chars.next() {
                    cleaned_line.push(next_c);
                }
                continue;
            }

            cleaned_line.push(c);
        }

        // Only trim trailing whitespace. Preserve leading indentation for YAML!
        let trimmed_end = cleaned_line.trim_end();
        result.push(trimmed_end.to_string());
    }
    result.join("\n")
}

fn strip_sql_comments(code: &str) -> String {
    let mut result = String::with_capacity(code.len());
    let mut chars = code.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '-' if chars.peek() == Some(&'-') => {
                chars.next();
                while let Some(&nc) = chars.peek() {
                    if nc == '\n' {
                        break;
                    }
                    chars.next();
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                while let Some(nc) = chars.next() {
                    if nc == '\n' {
                        result.push(nc);
                    }
                    if nc == '*' && chars.peek() == Some(&'/') {
                        chars.next();
                        break;
                    }
                }
            }
            '\'' => {
                result.push(c);
                while let Some(nc) = chars.next() {
                    result.push(nc);
                    if nc == '\'' {
                        break;
                    }
                }
            }
            _ => result.push(c),
        }
    }

    result
        .lines()
        .filter(|l| !l.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn strip_c_style_comments(code: &str) -> String {
    let mut result = String::with_capacity(code.len());
    let mut chars = code.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '/' => match chars.peek() {
                Some('/') => {
                    chars.next();
                    while let Some(&nc) = chars.peek() {
                        if nc == '\n' {
                            break;
                        }
                        chars.next();
                    }
                }
                Some('*') => {
                    chars.next();
                    while let Some(nc) = chars.next() {
                        if nc == '\n' {
                            result.push(nc);
                        }
                        if nc == '*' && chars.peek() == Some(&'/') {
                            chars.next();
                            break;
                        }
                    }
                }
                _ => result.push(c),
            },
            '"' => {
                result.push(c);
                while let Some(nc) = chars.next() {
                    result.push(nc);
                    if nc == '\\' {
                        if let Some(escaped) = chars.next() {
                            result.push(escaped);
                        }
                    } else if nc == '"' {
                        break;
                    }
                }
            }
            _ => result.push(c),
        }
    }

    result
        .lines()
        .filter(|l| !l.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_toml_comments() {
        let code = "[package]\nname = \"grab\" # my crate\n# version = \"0.1\"";
        let stripped = strip_comments(code, "toml");
        assert!(
            stripped.contains("name = \"grab\""),
            "Keep inline values. Got: {}",
            stripped
        );
        assert!(!stripped.contains("# my crate"), "Remove inline comments");
        assert!(!stripped.contains("# version"), "Remove full comment lines");
    }

    #[test]
    fn test_strip_sql_comments() {
        let code = "SELECT * -- comment\nFROM users;\n/* block \n comment */";
        let stripped = strip_comments(code, "sql");
        assert!(stripped.contains("SELECT *"), "Keep SQL. Got: {}", stripped);
        assert!(!stripped.contains("-- comment"), "Remove -- comments");
        assert!(!stripped.contains("/*"), "Remove block comments");
    }

    #[test]
    fn test_collapse_whitespace() {
        let code = "line 1\n\n\n\nline 2\n   \nline 3";
        let collapsed = collapse_whitespace(code);
        assert_eq!(
            collapsed, "line 1\n\nline 2\n\nline 3",
            "Should collapse to max 1 blank line. Got: {}",
            collapsed
        );
    }

    #[test]
    fn test_strip_yml_preserves_structure() {
        let code =
            "services:\n  db:\n    image: postgres # latest\n    # ports:\n    #   - 5432:5432";
        let stripped = strip_comments(code, "yml");
        assert!(
            stripped.contains("image: postgres"),
            "Keep values. Got: {}",
            stripped
        );
        assert!(!stripped.contains("# latest"), "Remove inline comments");
        assert!(!stripped.contains("# ports"), "Remove commented out code");
    }
}
