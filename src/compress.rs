use quote::ToTokens;
use syn::spanned::Spanned;
use syn::{File, Item};

use crate::stable_hash::stable_hash_body;

struct Compressor<'a> {
    file_path: &'a str,
    source_code: &'a str,
    hashes: Vec<(String, String, String, usize)>, // (hash, filepath, body, loc)
    skeleton: String,
}

impl<'a> Compressor<'a> {
    fn new(file_path: &'a str, source_code: &'a str) -> Self {
        Self {
            file_path,
            source_code,
            hashes: Vec::new(),
            skeleton: String::new(),
        }
    }

    fn hash_body(&self, body: &str) -> String {
        // Uses SHA-256 under the hood. 12 hex chars, deterministic.
        stable_hash_body(body, self.file_path)
    }

    /// Extract the exact source text for a span, preserving rustfmt formatting.
    /// Falls back to the compact token stream if span extraction fails.
    fn get_span_text(&self, span: proc_macro2::Span) -> Option<String> {
        let start = span.start();
        let end = span.end();

        if start.line == 0 || end.line == 0 || start.line > end.line {
            return None;
        }

        let lines: Vec<&str> = self.source_code.lines().collect();
        if start.line > lines.len() || end.line > lines.len() {
            return None;
        }

        let mut result = String::new();
        for i in start.line..=end.line {
            let line = lines.get(i - 1)?;
            if i == start.line && i == end.line {
                // Same line
                let end_bound = end.column.min(line.len());
                let start_bound = start.column.min(end_bound);
                result.push_str(&line[start_bound..end_bound]);
            } else if i == start.line {
                // First line of multi-line
                let start_bound = start.column.min(line.len());
                result.push_str(&line[start_bound..]);
                result.push('\n');
            } else if i == end.line {
                // Last line of multi-line
                let end_bound = end.column.min(line.len());
                result.push_str(&line[..end_bound]);
            } else {
                // Middle line
                result.push_str(line);
                result.push('\n');
            }
        }
        Some(result)
    }

    fn compress_item(&mut self, item: &Item) {
        match item {
            Item::Fn(f) => {
                let body = self
                    .get_span_text(f.block.span())
                    .unwrap_or_else(|| f.block.to_token_stream().to_string());
                let loc = body.lines().count();
                let hash = self.hash_body(&body);

                self.hashes
                    .push((hash.clone(), self.file_path.to_string(), body.clone(), loc));

                let skeleton_fn = if let Some(fn_text) = self.get_span_text(f.span()) {
                    if let Some(body_text) = self.get_span_text(f.block.span()) {
                        // Replace the exact body text with the hash stub
                        fn_text.replace(
                            &body_text,
                            &format!("{{ /* HASH:{} [{} LOC] */ }}", hash, loc),
                        )
                    } else {
                        // Fallback if body span extraction fails
                        let mut sig = f.to_token_stream().to_string();
                        let block_str = f.block.to_token_stream().to_string();
                        sig = sig.replace(
                            &block_str,
                            &format!("{{ /* HASH:{} [{} LOC] */ }}", hash, loc),
                        );
                        sig
                    }
                } else {
                    // Fallback if function span extraction fails
                    let mut sig = f.to_token_stream().to_string();
                    let block_str = f.block.to_token_stream().to_string();
                    sig = sig.replace(
                        &block_str,
                        &format!("{{ /* HASH:{} [{} LOC] */ }}", hash, loc),
                    );
                    sig
                };

                self.skeleton.push_str(&skeleton_fn);
                self.skeleton.push('\n');
            }
            Item::Impl(imp) => {
                let body = self
                    .get_span_text(imp.span())
                    .unwrap_or_else(|| imp.to_token_stream().to_string());
                let loc = body.lines().count();
                let hash = self.hash_body(&body);

                self.hashes
                    .push((hash.clone(), self.file_path.to_string(), body, loc));

                // Skeleton: keep the impl signature + item signatures, stub each fn body
                let mut skel_imp = imp.clone();
                for item in &mut skel_imp.items {
                    if let syn::ImplItem::Fn(f) = item {
                        let stub: syn::Block = syn::parse_quote!({});
                        f.block = stub;
                    }
                }

                // Prefix with the hash and LOC info
                self.skeleton
                    .push_str(&format!("/* HASH:{} [{} LOC] */\n", hash, loc));
                self.skeleton
                    .push_str(&skel_imp.to_token_stream().to_string());
                self.skeleton.push('\n');
            }
            Item::Struct(s) => {
                let body = self
                    .get_span_text(s.span())
                    .unwrap_or_else(|| s.to_token_stream().to_string());
                let loc = body.lines().count();
                let hash = self.hash_body(&body);

                self.hashes
                    .push((hash.clone(), self.file_path.to_string(), body, loc));
                self.skeleton.push_str(&format!(
                    "/* HASH:{} [{} LOC] (struct {}) */\n",
                    hash, loc, s.ident
                ));
            }
            Item::Enum(e) => {
                let body = self
                    .get_span_text(e.span())
                    .unwrap_or_else(|| e.to_token_stream().to_string());
                let loc = body.lines().count();
                let hash = self.hash_body(&body);

                self.hashes
                    .push((hash.clone(), self.file_path.to_string(), body, loc));
                self.skeleton.push_str(&format!(
                    "/* HASH:{} [{} LOC] (enum {}) */\n",
                    hash, loc, e.ident
                ));
            }
            Item::Trait(t) => {
                let body = self
                    .get_span_text(t.span())
                    .unwrap_or_else(|| t.to_token_stream().to_string());
                let loc = body.lines().count();
                let hash = self.hash_body(&body);

                self.hashes
                    .push((hash.clone(), self.file_path.to_string(), body, loc));
                self.skeleton.push_str(&format!(
                    "/* HASH:{} [{} LOC] (trait {}) */\n",
                    hash, loc, t.ident
                ));
            }
            // use, type aliases, consts, macros etc. — keep verbatim
            other => {
                let text = self
                    .get_span_text(other.span())
                    .unwrap_or_else(|| other.to_token_stream().to_string());
                self.skeleton.push_str(&text);
                self.skeleton.push('\n');
            }
        }
    }
}

/// Takes cleaned Rust code and a file path, returns extracted bodies with LOC
/// and a compressed skeleton string.
pub fn compress_code(
    code: &str,
    file_path: &str,
) -> (Vec<(String, String, String, usize)>, String) {
    let syntax_tree: File = match syn::parse_str(code) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Warning: syn parse failed for {}: {}", file_path, e);
            return (Vec::new(), code.to_string());
        }
    };

    let mut compressor = Compressor::new(file_path, code);

    for item in &syntax_tree.items {
        compressor.compress_item(item);
    }

    (compressor.hashes, compressor.skeleton)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_is_stable() {
        let code = "fn foo() { let x = 1; }";
        let (hashes1, _) = compress_code(code, "src/main.rs");
        let (hashes2, _) = compress_code(code, "src/main.rs");

        assert_eq!(hashes1.len(), 1);
        assert_eq!(
            hashes1[0].0, hashes2[0].0,
            "Hash must be stable across runs"
        );
    }

    #[test]
    fn test_different_files_different_hashes() {
        let code = "fn foo() { let x = 1; }";
        let (h1, _) = compress_code(code, "src/main.rs");
        let (h2, _) = compress_code(code, "src/lib.rs");

        assert_ne!(h1[0].0, h2[0].0, "Same body, different file must differ");
    }

    #[test]
    fn test_loc_tracking() {
        let code = r#"fn short() { 1 }

fn long() {
    let a = 1;
    let b = 2;
    let c = 3;
    a + b + c
}"#;
        let (hashes, _) = compress_code(code, "src/main.rs");

        assert_eq!(hashes.len(), 2);

        // short() block is `{ 1 }` -> 1 LOC
        // long() block spans 6 lines: {, a, b, c, a+b+c, }
        let short_hash = hashes
            .iter()
            .find(|h| h.3 == 1)
            .expect("Could not find 1 LOC block");
        let long_hash = hashes
            .iter()
            .find(|h| h.3 == 6)
            .expect("Could not find 6 LOC block");

        assert!(
            short_hash.3 < long_hash.3,
            "Long function must have more LOC"
        );
    }

    #[test]
    fn test_skeleton_format() {
        let code = r#"struct MyStruct;

impl MyStruct {
    fn do_thing(&self) { /* body */ }
}"#;
        let (_, skeleton) = compress_code(code, "src/models.rs");

        // Struct is 1 line
        assert!(
            skeleton.contains("[1 LOC] (struct MyStruct)"),
            "Struct skeleton missing LOC. Got: {}",
            skeleton
        );

        // Impl is 3 lines total, and it's hashed as a single block
        assert!(
            skeleton.contains("[3 LOC]"),
            "Impl skeleton missing LOC. Got: {}",
            skeleton
        );

        // The inner fn inside the impl should have its original body removed
        assert!(
            !skeleton.contains("/* body */"),
            "Fn inside impl should have its body removed. Got: {}",
            skeleton
        );
    }

    #[test]
    fn test_parse_failure_fallback() {
        let bad_code = "fn missing_brace() {";
        let (hashes, skeleton) = compress_code(bad_code, "src/bad.rs");

        assert!(hashes.is_empty(), "Bad parse should yield no hashes");
        assert_eq!(
            skeleton, bad_code,
            "Bad parse should return raw code as skeleton"
        );
    }

    #[test]
    fn test_body_preserves_formatting() {
        let code = r#"fn formatted() {
    let x = 1;
    let y = 2;
    x + y
}"#;
        let (hashes, _) = compress_code(code, "src/main.rs");
        assert_eq!(hashes.len(), 1);

        let body = &hashes[0].2;
        // Ensure the extracted body kept the newlines and formatting
        assert!(
            body.contains("    let x = 1;"),
            "Body must preserve formatting. Got: {}",
            body
        );

        // The body includes the braces on their own lines, so it's 5 LOC:
        // 1: {
        // 2:     let x = 1;
        // 3:     let y = 2;
        // 4:     x + y
        // 5: }
        assert!(
            body.lines().count() == 5,
            "Body LOC must be 5. Got: {}",
            body.lines().count()
        );
    }
}
