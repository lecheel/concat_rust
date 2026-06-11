//--+ src/strip.rs

/// Remove Rust comments (// and /* */), preserving strings.
pub fn remove_rust_comments(code: &str) -> String {
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
}

/// Remove `#[cfg(test)]` and `mod tests { ... }` blocks.
pub fn remove_test_modules(code: &str) -> String {
    let lines: Vec<&str> = code.lines().collect();
    let mut result = Vec::new();
    let n = lines.len();
    let mut i = 0;

    while i < n {
        let line = lines[i];
        let stripped = line.trim();
        let mut is_test_start = false;

        if stripped.starts_with("mod test") || stripped.starts_with("mod revert") {
            if line.contains('{') {
                is_test_start = true;
            } else {
                let mut j = i + 1;
                while j < n && lines[j].trim().is_empty() {
                    j += 1;
                }
                if j < n && lines[j].contains('{') {
                    is_test_start = true;
                }
            }
        } else if stripped.starts_with("#[cfg(test)]") {
            let mut j = i + 1;
            while j < n && lines[j].trim().is_empty() {
                j += 1;
            }
            if j < n && lines[j].trim().starts_with("mod ") {
                is_test_start = true;
            }
        }

        if is_test_start {
            let mut brace_line_idx = i;
            if !lines[brace_line_idx].contains('{') {
                for k in brace_line_idx..n {
                    if lines[k].contains('{') {
                        brace_line_idx = k;
                        break;
                    }
                }
            }

            let mut brace_count = 0i32;
            let mut j = brace_line_idx;
            while j < n {
                for ch in lines[j].chars() {
                    match ch {
                        '{' => brace_count += 1,
                        '}' => brace_count -= 1,
                        _ => {}
                    }
                }
                if j >= brace_line_idx && brace_count == 0 {
                    j += 1;
                    break;
                }
                j += 1;
            }
            i = j;
        } else {
            result.push(line);
            i += 1;
        }
    }

    result.join("\n")
}

/// Remove empty lines.
pub fn remove_empty_lines(text: &str) -> String {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_remove_line_comments() {
        let code = "let x = 1; // comment\n// whole line\nlet y = 2;";
        let stripped = remove_rust_comments(code);
        assert!(
            stripped.contains("let x = 1;"),
            "Keep code. Got: {}",
            stripped
        );
        assert!(!stripped.contains("// comment"), "Remove inline comment");
        assert!(
            !stripped.contains("// whole line"),
            "Remove whole line comment"
        );
        assert!(stripped.contains("let y = 2;"), "Keep code after comment");
    }

    #[test]
    fn test_remove_block_comments() {
        let code = "let x = /* block */ 1;";
        let stripped = remove_rust_comments(code);
        assert_eq!(
            stripped, "let x =  1;",
            "Remove block comment. Got: {}",
            stripped
        );
    }

    #[test]
    fn test_preserves_strings() {
        let code = r#"let s = "http://example.com"; // real url"#;
        let stripped = remove_rust_comments(code);
        assert!(
            stripped.contains(r#""http://example.com""#),
            "Preserve string contents. Got: {}",
            stripped
        );
        assert!(
            !stripped.contains("// real url"),
            "Remove comment after string"
        );
    }

    #[test]
    fn test_remove_test_modules() {
        let code =
            "fn real() {}\n\n#[cfg(test)]\nmod tests {\n    fn test_thing() {}\n}\nfn other() {}";
        let stripped = remove_test_modules(code);
        assert!(stripped.contains("fn real() {}"), "Keep real code");
        assert!(stripped.contains("fn other() {}"), "Keep code after tests");
        assert!(
            !stripped.contains("mod tests"),
            "Remove test module declaration"
        );
        assert!(
            !stripped.contains("test_thing"),
            "Remove test module contents"
        );
    }

    #[test]
    fn test_remove_empty_lines() {
        let code = "a\n\n\nb\n   \nc";
        let stripped = remove_empty_lines(code);
        assert_eq!(
            stripped, "a\nb\nc",
            "Should remove empty lines. Got: {}",
            stripped
        );
    }
}
