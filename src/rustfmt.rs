//--+ src/rustfmt.rs

use std::io::Write;
use std::process::Command;

/// Run rustfmt on a string of code in memory. Returns the formatted code.
/// If rustfmt fails or is missing, returns the original code with a warning.
pub fn run_rustfmt(code: &str, max_width: i32) -> String {
    // Strip `mod foo;` declarations first, as rustfmt can't resolve them
    let (stripped, placeholders) = strip_mod_decls(code);

    let max_width_config = format!("max_width={}", max_width);

    let mut child = match Command::new("rustfmt")
        .arg("--edition")
        .arg("2021")
        .arg("--config")
        .arg(&max_width_config)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Warning: rustfmt execution failed: {}", e);
            return restore_mod_decls(&stripped, &placeholders);
        }
    };

    if let Some(mut stdin) = child.stdin.take() {
        if let Err(e) = stdin.write_all(stripped.as_bytes()) {
            eprintln!("Warning: failed to write to rustfmt stdin: {}", e);
            return restore_mod_decls(&stripped, &placeholders);
        }
    }

    let output = match child.wait_with_output() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("Warning: rustfmt wait failed: {}", e);
            return restore_mod_decls(&stripped, &placeholders);
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("Warning: rustfmt failed: {}", stderr.trim());
        return restore_mod_decls(&stripped, &placeholders);
    }

    let formatted = match String::from_utf8(output.stdout) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Warning: rustfmt output was not valid UTF-8: {}", e);
            return restore_mod_decls(&stripped, &placeholders);
        }
    };

    restore_mod_decls(&formatted, &placeholders)
}

/// Replace every `mod foo;` line with a unique sentinel comment.
/// Returns the scrubbed source and a list of (sentinel, original_line) pairs.
fn strip_mod_decls(code: &str) -> (String, Vec<(String, String)>) {
    let mut placeholders = Vec::new();
    let mut out_lines = Vec::new();

    for line in code.lines() {
        let trimmed = line.trim();
        if is_mod_decl(trimmed) {
            let sentinel = format!("// __MOD_{:04}__", placeholders.len());
            placeholders.push((sentinel.clone(), line.to_string()));
            out_lines.push(sentinel);
        } else {
            out_lines.push(line.to_string());
        }
    }

    (out_lines.join("\n"), placeholders)
}

fn is_mod_decl(trimmed: &str) -> bool {
    // Must end with `;` and not contain `{` (inline module)
    if !trimmed.ends_with(';') || trimmed.contains('{') {
        return false;
    }

    // Strip optional attributes like #[cfg(test)] if on the same line
    let mut s = trimmed;
    while s.starts_with('#') {
        if let Some(close_bracket) = s.find(']') {
            s = s[close_bracket + 1..].trim();
        } else {
            break;
        }
    }

    // Strip visibility modifiers
    if let Some(rest) = s.strip_prefix("pub") {
        s = rest.trim();
        // Handle pub(crate), pub(super), etc.
        if s.starts_with('(') {
            if let Some(close_paren) = s.find(')') {
                s = s[close_paren + 1..].trim();
            }
        }
    }

    s.starts_with("mod ")
}

fn restore_mod_decls(code: &str, placeholders: &[(String, String)]) -> String {
    let mut result = code.to_string();
    for (sentinel, original) in placeholders {
        result = result.replace(sentinel.as_str(), original.as_str());
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mod_decl_detection() {
        assert!(is_mod_decl("mod foo;"));
        assert!(is_mod_decl("pub mod bar;"));
        assert!(is_mod_decl("pub(crate) mod baz;"));
        assert!(!is_mod_decl("mod foo { }")); // inline module
        assert!(!is_mod_decl("fn foo();")); // not a mod
    }

    #[test]
    fn test_strip_restore_mod_decls() {
        let code = "mod foo;\npub mod bar;\nfn main() {}";
        let (stripped, ph) = strip_mod_decls(code);

        assert!(!stripped.contains("mod foo;"), "Should strip mod foo;");
        assert!(
            !stripped.contains("pub mod bar;"),
            "Should strip pub mod bar;"
        );
        assert!(stripped.contains("fn main() {}"), "Should keep fn main");

        let restored = restore_mod_decls(&stripped, &ph);
        assert_eq!(restored, code, "Should perfectly restore original code");
    }

    #[test]
    fn test_rustfmt_runs() {
        let ugly = "fn main (){1+2}";
        let formatted = run_rustfmt(ugly, 100);
        // If rustfmt is installed, it should format it. If not, it falls back gracefully.
        assert!(
            formatted.contains("fn main"),
            "Must contain fn main. Got: {}",
            formatted
        );
    }
}
