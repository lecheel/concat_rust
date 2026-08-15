use crate::stable_hash::stable_hash_body;
use std::io::Write;
use std::process::{Command, Stdio};

/// Runs `gofmt` on the provided Go source code.
/// If `gofmt` is not installed or fails, it returns the original code.
pub fn run_gofmt(code: &str) -> String {
    let mut child = match Command::new("gofmt")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return code.to_string(), // fallback if gofmt isn't installed
    };

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(code.as_bytes());
    }

    let output = match child.wait_with_output() {
        Ok(o) => o,
        Err(_) => return code.to_string(),
    };

    if output.status.success() {
        String::from_utf8_lossy(&output.stdout).to_string()
    } else {
        code.to_string()
    }
}

/// Strips // and /* */ comments from Go code while respecting strings and runes.
pub fn strip_go_comments(code: &str) -> String {
    let mut result = String::new();
    let bytes = code.as_bytes();
    let n = bytes.len();
    let mut i = 0usize;

    let mut in_line_comment = false;
    let mut in_block_comment = false;
    let mut in_string = false;
    let mut string_delim: u8 = 0;
    let mut in_rune = false;

    while i < n {
        let c = bytes[i];

        if in_line_comment {
            if c == b'\n' {
                in_line_comment = false;
                result.push('\n');
            }
            i += 1;
            continue;
        }
        if in_block_comment {
            if c == b'*' && i + 1 < n && bytes[i + 1] == b'/' {
                in_block_comment = false;
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }
        if in_string {
            result.push(c as char);
            if c == b'\\' && string_delim == b'"' && i + 1 < n {
                result.push(bytes[i + 1] as char);
                i += 2;
                continue;
            }
            if c == string_delim || (c == b'\n' && string_delim == b'"') {
                in_string = false;
            }
            i += 1;
            continue;
        }
        if in_rune {
            result.push(c as char);
            if c == b'\\' && i + 1 < n {
                result.push(bytes[i + 1] as char);
                i += 2;
                continue;
            }
            if c == b'\'' {
                in_rune = false;
            }
            i += 1;
            continue;
        }

        if c == b'/' && i + 1 < n {
            if bytes[i + 1] == b'/' {
                in_line_comment = true;
                i += 2;
                continue;
            }
            if bytes[i + 1] == b'*' {
                in_block_comment = true;
                i += 2;
                continue;
            }
        }
        if c == b'"' {
            in_string = true;
            string_delim = b'"';
            result.push('"');
            i += 1;
            continue;
        }
        if c == b'`' {
            in_string = true;
            string_delim = b'`';
            result.push('`');
            i += 1;
            continue;
        }
        if c == b'\'' {
            in_rune = true;
            result.push('\'');
            i += 1;
            continue;
        }

        result.push(c as char);
        i += 1;
    }
    result
}

/// Compress a Go source file: extract function bodies into hashes and
/// produce a skeleton with stubs.
pub fn compress_go_code(
    source_code: &str,
    file_path: &str,
) -> (Vec<(String, String, String, usize)>, String) {
    let mut hashes: Vec<(String, String, String, usize)> = Vec::new();
    let mut skeleton = String::new();

    let bytes = source_code.as_bytes();
    let n = bytes.len();
    let mut i = 0usize;

    let mut brace_depth: i32 = 0;
    let mut paren_depth: i32 = 0;
    let mut at_line_start = true;
    let mut in_line_comment = false;
    let mut in_block_comment = false;
    let mut in_string = false;
    let mut string_delim: u8 = 0;
    let mut in_rune = false;

    while i < n {
        let c = bytes[i];

        if in_line_comment {
            skeleton.push(c as char);
            if c == b'\n' {
                in_line_comment = false;
                at_line_start = true;
            }
            i += 1;
            continue;
        }
        if in_block_comment {
            skeleton.push(c as char);
            if c == b'*' && i + 1 < n && bytes[i + 1] == b'/' {
                skeleton.push('/');
                i += 2;
                continue;
            }
            if c == b'\n' {
                at_line_start = true;
            }
            i += 1;
            continue;
        }
        if in_string {
            skeleton.push(c as char);
            if c == b'\\' && string_delim == b'"' && i + 1 < n {
                skeleton.push(bytes[i + 1] as char);
                i += 2;
                continue;
            }
            if c == string_delim {
                in_string = false;
            }
            if c == b'\n' && string_delim == b'"' {
                in_string = false;
                at_line_start = true;
            }
            i += 1;
            continue;
        }
        if in_rune {
            skeleton.push(c as char);
            if c == b'\\' && i + 1 < n {
                skeleton.push(bytes[i + 1] as char);
                i += 2;
                continue;
            }
            if c == b'\'' {
                in_rune = false;
            }
            i += 1;
            continue;
        }

        if c == b'/' && i + 1 < n {
            if bytes[i + 1] == b'/' {
                in_line_comment = true;
                skeleton.push_str("//");
                i += 2;
                continue;
            }
            if bytes[i + 1] == b'*' {
                in_block_comment = true;
                skeleton.push_str("/*");
                i += 2;
                continue;
            }
        }
        if c == b'"' {
            in_string = true;
            string_delim = b'"';
            skeleton.push('"');
            at_line_start = false;
            i += 1;
            continue;
        }
        if c == b'`' {
            in_string = true;
            string_delim = b'`';
            skeleton.push('`');
            at_line_start = false;
            i += 1;
            continue;
        }
        if c == b'\'' {
            in_rune = true;
            skeleton.push('\'');
            at_line_start = false;
            i += 1;
            continue;
        }

        if c == b'\n' {
            skeleton.push('\n');
            at_line_start = true;
            i += 1;
            continue;
        }
        if at_line_start && (c == b' ' || c == b'\t' || c == b'\r') {
            skeleton.push(c as char);
            i += 1;
            continue;
        }

        if at_line_start {
            at_line_start = false;
            if brace_depth == 0
                && paren_depth == 0
                && c == b'f'
                && i + 4 <= n
                && &source_code[i..i + 4] == "func"
            {
                let after = if i + 4 < n { bytes[i + 4] } else { b' ' };
                if after == b' ' || after == b'\t' || after == b'(' || after == b'\n' {
                    if let Some((open_brace, close_brace)) = find_go_func_body(source_code, i + 4) {
                        let body = &source_code[open_brace..=close_brace];
                        let loc = body.lines().count();
                        let hash = stable_hash_body(body, file_path);
                        hashes.push((hash.clone(), file_path.to_string(), body.to_string(), loc));

                        skeleton.push_str(&source_code[i..=open_brace]);
                        skeleton.push_str(&format!(" /* HASH:{} [{} LOC] */ ", hash, loc));
                        skeleton.push('}');

                        i = close_brace + 1;
                        continue;
                    }
                }
            }
        }

        if c == b'{' {
            brace_depth += 1;
        } else if c == b'}' {
            brace_depth -= 1;
        } else if c == b'(' {
            paren_depth += 1;
        } else if c == b')' {
            paren_depth -= 1;
        }

        skeleton.push(c as char);
        i += 1;
    }

    (hashes, skeleton)
}

fn find_go_func_body(source: &str, start: usize) -> Option<(usize, usize)> {
    let bytes = source.as_bytes();
    let n = bytes.len();
    let mut i = start;

    let mut paren_depth: i32 = 0;
    let mut in_line_comment = false;
    let mut in_block_comment = false;
    let mut in_string = false;
    let mut string_delim: u8 = 0;
    let mut in_rune = false;

    while i < n {
        let c = bytes[i];

        if in_line_comment {
            if c == b'\n' {
                in_line_comment = false;
            }
            i += 1;
            continue;
        }
        if in_block_comment {
            if c == b'*' && i + 1 < n && bytes[i + 1] == b'/' {
                in_block_comment = false;
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }
        if in_string {
            if c == b'\\' && string_delim == b'"' {
                i += 2;
                continue;
            }
            if c == string_delim || (c == b'\n' && string_delim == b'"') {
                in_string = false;
            }
            i += 1;
            continue;
        }
        if in_rune {
            if c == b'\\' {
                i += 2;
                continue;
            }
            if c == b'\'' {
                in_rune = false;
            }
            i += 1;
            continue;
        }

        if c == b'/' && i + 1 < n {
            if bytes[i + 1] == b'/' {
                in_line_comment = true;
                i += 2;
                continue;
            }
            if bytes[i + 1] == b'*' {
                in_block_comment = true;
                i += 2;
                continue;
            }
        }
        if c == b'"' {
            in_string = true;
            string_delim = b'"';
            i += 1;
            continue;
        }
        if c == b'`' {
            in_string = true;
            string_delim = b'`';
            i += 1;
            continue;
        }
        if c == b'\'' {
            in_rune = true;
            i += 1;
            continue;
        }

        if c == b'(' {
            paren_depth += 1;
        } else if c == b')' {
            paren_depth -= 1;
        } else if c == b'{' && paren_depth == 0 {
            return find_matching_brace(source, i);
        }

        i += 1;
    }

    None
}

fn find_matching_brace(source: &str, open: usize) -> Option<(usize, usize)> {
    let bytes = source.as_bytes();
    let n = bytes.len();
    let mut j = open + 1;
    let mut depth: i32 = 1;

    let mut in_line_comment = false;
    let mut in_block_comment = false;
    let mut in_string = false;
    let mut string_delim: u8 = 0;
    let mut in_rune = false;

    while j < n && depth > 0 {
        let c = bytes[j];

        if in_line_comment {
            if c == b'\n' {
                in_line_comment = false;
            }
            j += 1;
            continue;
        }
        if in_block_comment {
            if c == b'*' && j + 1 < n && bytes[j + 1] == b'/' {
                in_block_comment = false;
                j += 2;
                continue;
            }
            j += 1;
            continue;
        }
        if in_string {
            if c == b'\\' && string_delim == b'"' {
                j += 2;
                continue;
            }
            if c == string_delim || (c == b'\n' && string_delim == b'"') {
                in_string = false;
            }
            j += 1;
            continue;
        }
        if in_rune {
            if c == b'\\' {
                j += 2;
                continue;
            }
            if c == b'\'' {
                in_rune = false;
            }
            j += 1;
            continue;
        }

        if c == b'/' && j + 1 < n {
            if bytes[j + 1] == b'/' {
                in_line_comment = true;
                j += 2;
                continue;
            }
            if bytes[j + 1] == b'*' {
                in_block_comment = true;
                j += 2;
                continue;
            }
        }

        if c == b'"' {
            in_string = true;
            string_delim = b'"';
        } else if c == b'`' {
            in_string = true;
            string_delim = b'`';
        } else if c == b'\'' {
            in_rune = true;
        } else if c == b'{' {
            depth += 1;
        } else if c == b'}' {
            depth -= 1;
            if depth == 0 {
                return Some((open, j));
            }
        }

        j += 1;
    }

    None
}
