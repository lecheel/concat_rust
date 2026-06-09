use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use axum::{extract::Path as AxumPath, routing::get, Router};
use clap::Parser;
// use regex::Regex;
use serde::{Deserialize, Serialize};
// use sha2::{Digest, Sha256};
use std::hash::DefaultHasher;
use std::hash::{Hash, Hasher};
use tokio::sync::Mutex;

use quote::ToTokens;
use syn::{File, Item};

struct Compressor<'a> {
    file_path: &'a str,
    hashes: Vec<(String, String, String)>, // (hash, filepath, body)
    skeleton: String,
    used_short_hashes: HashMap<String, usize>,
}

impl<'a> Compressor<'a> {
    fn new(file_path: &'a str) -> Self {
        Self {
            file_path,
            hashes: Vec::new(),
            skeleton: String::new(),
            used_short_hashes: HashMap::new(),
        }
    }

    fn hash_body(&mut self, body: &str) -> String {
        let mut hasher = DefaultHasher::new();
        body.hash(&mut hasher);
        let full_hash = hasher.finish(); // u64
                                         // Take the first 8 hex digits of the 16-digit representation
        let base = format!("{:016x}", full_hash)[..8].to_string();

        match self.used_short_hashes.get(&base).copied() {
            Some(count) => {
                self.used_short_hashes.insert(base.clone(), count + 1);
                format!("{}_{}", base, count + 1)
            }
            None => {
                self.used_short_hashes.insert(base.clone(), 1);
                base
            }
        }
    }

    fn compress_item(&mut self, item: &Item) {
        match item {
            Item::Fn(f) => {
                let body = f.block.to_token_stream().to_string();
                let hash = self.hash_body(&body);
                self.hashes
                    .push((hash.clone(), self.file_path.to_string(), body));

                let mut sig = f.to_token_stream().to_string();
                // replace the block with the hash stub
                let block_str = f.block.to_token_stream().to_string();
                sig = sig.replace(&block_str, &format!("{{ /* HASH:{} */ }}", hash));
                self.skeleton.push_str(&sig);
                self.skeleton.push('\n');
            }
            Item::Impl(imp) => {
                let hash = {
                    let body = imp.to_token_stream().to_string();
                    self.hash_body(&body)
                };
                self.hashes.push((
                    hash.clone(),
                    self.file_path.to_string(),
                    imp.to_token_stream().to_string(),
                ));
                // skeleton: keep the impl signature + item signatures, stub each fn body
                let mut skel_imp = imp.clone();
                for item in &mut skel_imp.items {
                    if let syn::ImplItem::Fn(f) = item {
                        let stub: syn::Block = syn::parse_quote!({ /* stubbed */ });
                        f.block = stub;
                    }
                }
                self.skeleton
                    .push_str(&skel_imp.to_token_stream().to_string());
                self.skeleton.push('\n');
            }
            Item::Struct(s) => {
                let body = s.to_token_stream().to_string();
                let hash = self.hash_body(&body);
                self.hashes
                    .push((hash.clone(), self.file_path.to_string(), body.clone()));
                self.skeleton
                    .push_str(&format!("/* HASH:{} (struct {}) */\n", hash, s.ident));
            }
            Item::Enum(e) => {
                let body = e.to_token_stream().to_string();
                let hash = self.hash_body(&body);
                self.hashes
                    .push((hash.clone(), self.file_path.to_string(), body.clone()));
                self.skeleton
                    .push_str(&format!("/* HASH:{} (enum {}) */\n", hash, e.ident));
            }
            Item::Trait(t) => {
                let body = t.to_token_stream().to_string();
                let hash = self.hash_body(&body);
                self.hashes
                    .push((hash.clone(), self.file_path.to_string(), body.clone()));
                self.skeleton
                    .push_str(&format!("/* HASH:{} (trait {}) */\n", hash, t.ident));
            }
            // use, type aliases, consts, macros etc. — keep verbatim
            other => {
                self.skeleton.push_str(&other.to_token_stream().to_string());
                self.skeleton.push('\n');
            }
        }
    }
}

fn compress_code(code: &str, file_path: &str) -> (Vec<(String, String, String)>, String) {
    let syntax_tree: File = match syn::parse_str(code) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Warning: syn parse failed for {}: {}", file_path, e);
            // fall back: return code as-is, no compression
            return (Vec::new(), code.to_string());
        }
    };

    let mut compressor = Compressor::new(file_path);

    // preserve top-level attributes and use items verbatim
    for item in &syntax_tree.items {
        compressor.compress_item(item);
    }

    (compressor.hashes, compressor.skeleton)
}

/// Run rustfmt first on each file, then remove tests/comments/empty lines.
#[derive(Parser, Debug)]
#[command(
    name = "concat_rust",
    about = "Run rustfmt first on each file, then remove tests/comments/empty lines."
)]
struct Args {
    /// Source directory
    #[arg(long, default_value = "src")]
    dir: String,

    /// Output file
    #[arg(long, default_value = "output.rs")]
    output: String,

    /// rustfmt max line width
    #[arg(long, default_value_t = 350)]
    max_width: i32,

    /// Skip rustfmt (no formatting first)
    #[arg(long)]
    no_format: bool,

    /// Join everything into one line after cleanup
    #[arg(long)]
    single_line: bool,

    /// Compress bodies into hashes and start a daemon to retrieve them
    #[arg(long)]
    compress: bool,

    /// Port for the retrieval daemon (requires --compress)
    #[arg(long, default_value_t = 7890)]
    daemon_port: u16,

    /// Resume daemon from existing skeleton cache (skips processing files)
    #[arg(long)]
    resume: bool,
}

// ------------------------------------------------------------
// Helper: run rustfmt
// ------------------------------------------------------------
fn run_rustfmt(code: &str, max_width: i32, original_path: &Path) -> String {
    let pid = std::process::id();

    let tmp_dir = match original_path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    let in_path = tmp_dir.join(format!(".concat_rust_{}.rs", pid));
    let cfg_path = std::env::temp_dir().join(format!("concat_rust_{}.toml", pid));

    if let Err(e) = fs::write(&in_path, code) {
        eprintln!("Warning: could not write temp file for rustfmt: {}", e);
        return code.to_string();
    }

    let config = format!("max_width = {}\n", max_width);
    if let Err(e) = fs::write(&cfg_path, &config) {
        eprintln!("Warning: could not write rustfmt config: {}", e);
        let _ = fs::remove_file(&in_path);
        return code.to_string();
    }

    let result = Command::new("rustfmt")
        .arg("--edition")
        .arg("2021")
        .arg("--config-path")
        .arg(&cfg_path)
        .arg(&in_path)
        .output();

    let formatted = match result {
        Ok(output) => {
            if output.status.success() {
                fs::read_to_string(&in_path).unwrap_or_else(|e| {
                    eprintln!("Warning: failed to read rustfmt output: {}", e);
                    code.to_string()
                })
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                eprintln!("Warning: rustfmt failed:\n{}", stderr.trim());
                code.to_string()
            }
        }
        Err(e) => {
            eprintln!("Warning: rustfmt execution failed: {}", e);
            code.to_string()
        }
    };

    let _ = fs::remove_file(&in_path);
    let _ = fs::remove_file(&cfg_path);

    formatted
}

// ------------------------------------------------------------
// Cleanup functions
// ------------------------------------------------------------
fn remove_test_modules(code: &str) -> String {
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

fn remove_rust_comments(code: &str, _file_path: &str) -> String {
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

fn remove_empty_lines(text: &str) -> String {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn find_rs_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                files.extend(find_rs_files(&path));
            } else if path.extension().map_or(false, |ext| ext == "rs") {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

// ------------------------------------------------------------
// Daemon Cache & State
// ------------------------------------------------------------
#[derive(Serialize, Deserialize, Clone, Default)]
struct DaemonCache {
    bodies: HashMap<String, (String, String)>,
    files: HashMap<String, String>,
    #[serde(default)]
    skeleton: String,
}

#[derive(Clone)]
struct AppState {
    cache: Arc<Mutex<DaemonCache>>,
}

async fn get_body(
    AxumPath(prefix): AxumPath<String>,
    state: axum::extract::State<AppState>,
) -> impl axum::response::IntoResponse {
    let db = state.cache.lock().await;

    // Split the query path by '+' or ',' to process multiple requested hashes.
    let hashes: Vec<&str> = prefix
        .split(|c| c == '+' || c == ',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    if hashes.is_empty() {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            "No hashes provided".to_string(),
        );
    }

    if hashes.len() == 1 {
        let single_hash = hashes[0];

        // 1. Exact match
        if let Some((filepath, body)) = db.bodies.get(single_hash) {
            return (
                axum::http::StatusCode::OK,
                format!("//--+ file:///{}\n{}", filepath, body),
            );
        }

        // 2. Prefix match
        let matches: Vec<(&String, &(String, String))> = db
            .bodies
            .iter()
            .filter(|(hash, _)| hash.starts_with(single_hash))
            .collect();

        match matches.len() {
            0 => (
                axum::http::StatusCode::NOT_FOUND,
                format!("No hash found matching prefix '{}'", single_hash),
            ),
            1 => {
                let (hash, (filepath, body)) = matches[0];
                (
                    axum::http::StatusCode::OK,
                    format!("//--+ file:///{}\n// Hash: {}\n{}", filepath, hash, body),
                )
            }
            _ => {
                let list: Vec<String> = matches
                    .iter()
                    .map(|(h, (f, _))| format!("  {}  ({})", h, f))
                    .collect();
                (
                    axum::http::StatusCode::CONFLICT,
                    format!(
                        "Ambiguous prefix '{}' matches {} hashes:\n{}\nSend more characters to disambiguate.",
                        single_hash,
                        matches.len(),
                        list.join("\n")
                    ),
                )
            }
        }
    } else {
        // Multi-hash requests (e.g., hash1+hash2)
        let mut results = Vec::new();
        let mut not_found = Vec::new();
        let mut ambiguous = Vec::new();

        for h in hashes {
            // Check exact match first
            if let Some((filepath, body)) = db.bodies.get(h) {
                results.push(format!(
                    "//--+ file:///{}\n// Hash: {}\n{}",
                    filepath, h, body
                ));
                continue;
            }

            // Check prefix match
            let matches: Vec<(&String, &(String, String))> = db
                .bodies
                .iter()
                .filter(|(hash, _)| hash.starts_with(h))
                .collect();

            match matches.len() {
                0 => not_found.push(h.to_string()),
                1 => {
                    let (full_hash, (filepath, body)) = matches[0];
                    results.push(format!(
                        "//--+ file:///{}\n// Hash: {}\n{}",
                        filepath, full_hash, body
                    ));
                }
                _ => {
                    let list: Vec<String> = matches
                        .iter()
                        .map(|(fh, (f, _))| format!("  {} ({})", fh, f))
                        .collect();
                    ambiguous.push(format!("'{}' matches:\n{}", h, list.join("\n")));
                }
            }
        }

        if !not_found.is_empty() || !ambiguous.is_empty() {
            let mut err_msg = String::new();
            if !not_found.is_empty() {
                err_msg.push_str(&format!("Hashes not found: {}\n", not_found.join(", ")));
            }
            if !ambiguous.is_empty() {
                err_msg.push_str(&format!("Ambiguous prefixes:\n{}", ambiguous.join("\n")));
            }
            return (axum::http::StatusCode::BAD_REQUEST, err_msg);
        }

        (axum::http::StatusCode::OK, results.join("\n\n"))
    }
}

async fn get_file(
    AxumPath(path): AxumPath<String>,
    state: axum::extract::State<AppState>,
) -> impl axum::response::IntoResponse {
    let db = state.cache.lock().await;
    if let Some(code) = db.files.get(&path) {
        (
            axum::http::StatusCode::OK,
            format!("//--+ file:///{}\n{}", path, code),
        )
    } else {
        (
            axum::http::StatusCode::NOT_FOUND,
            format!("File {} not found", path),
        )
    }
}

async fn get_skeleton(state: axum::extract::State<AppState>) -> impl axum::response::IntoResponse {
    let db = state.cache.lock().await;
    if db.skeleton.is_empty() {
        (
            axum::http::StatusCode::NOT_FOUND,
            "No skeleton available".to_string(),
        )
    } else {
        (axum::http::StatusCode::OK, db.skeleton.clone())
    }
}

// ------------------------------------------------------------
// Main
// ------------------------------------------------------------
#[tokio::main]
async fn main() {
    let args = Args::parse();

    let cache_path = format!("{}.cache", args.output);
    let mut cache = DaemonCache::default();

    if args.resume {
        if !PathBuf::from(&cache_path).exists() {
            eprintln!(
                "Error: Cache file '{}' not found. Run without --resume first to generate it.",
                cache_path
            );
            std::process::exit(1);
        }
        println!("Resuming daemon from cache: {}", cache_path);
        let cache_data = fs::read_to_string(&cache_path).expect("Failed to read cache file");
        cache = serde_json::from_str(&cache_data).expect("Failed to parse cache file");
    } else {
        let src_dir = PathBuf::from(&args.dir);
        if !src_dir.is_dir() {
            eprintln!("Error: {} not found", args.dir);
            std::process::exit(1);
        }

        let rust_files = find_rs_files(&src_dir);
        if rust_files.is_empty() {
            println!("No .rs files found.");
            return;
        }

        let mut parts = Vec::new();

        for fp in &rust_files {
            let rel = fp.strip_prefix(&src_dir).unwrap_or(fp.as_path());
            let rel_str = rel.to_string_lossy().to_string();

            let raw_code = match fs::read_to_string(fp) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Warning: could not read {}: {}", fp.display(), e);
                    continue;
                }
            };

            let formatted = if !args.no_format {
                run_rustfmt(&raw_code, args.max_width, fp)
            } else {
                raw_code
            };

            let no_comments = remove_rust_comments(&formatted, &rel_str);
            let no_tests = remove_test_modules(&no_comments);
            let no_empty = remove_empty_lines(&no_tests);

            if args.compress {
                let (hashes, skeleton) = compress_code(&no_empty, &rel_str);
                for (hash, filepath, body) in hashes {
                    cache.bodies.insert(hash, (filepath, body));
                }
                cache.files.insert(rel_str.clone(), no_empty);
                // Prepend `file:///` scheme to the separator
                parts.push(format!("//--+ file:///{}\n{}", rel_str, skeleton));
            } else {
                parts.push(format!("//--+ file:///{}\n{}", rel_str, no_empty));
            }
        }

        let mut combined = parts.join("\n");

        if !args.compress && args.single_line {
            combined = join_into_one_line(&combined);
        }

        if args.compress {
            let header = format!(
                "// === SKELETON MODE (COMPRESSED) ===\n\
                 // To retrieve the full implementation of a function/struct/impl block,\n\
                 // make an HTTP GET request to http://localhost:{}/<HASH>\n\
                 // To retrieve multiple blocks at once, use '+' or ',' separator: http://localhost:{}/<HASH1>+<HASH2>\n\
                 // To retrieve a whole file, request http://localhost:{}/file/<FILEPATH>\n\
                 // To retrieve this skeleton, request http://localhost:{}/skeleton\n\
                 // ===================================\n\n",
                args.daemon_port, args.daemon_port, args.daemon_port, args.daemon_port
            );

            let meta_prompt = format!(
                "\n\n===\n\
                 Your process:\n\
                 Analyze the feature requirements (provided after this meta‑prompt) and determine which files, structs, traits, and functions you need to see the full implementation of.\n\
                 Prefer asking for whole files rather than individual hashes, because a file may contain many interconnected hashes.\n\
                 If a file is too large, you may ask for specific impl blocks or struct definitions by their HASH, but state clearly that you need the surrounding context.\n\
                 List exactly what you need in a clear, numbered list. For each item, include:\n\
                 The file path (e.g., models/application/modes/open/mod.rs).\n\
                 If you need a specific block, include its HASH (e.g., /* HASH:1a12fb93 */ for the OpenMode struct).\n\
                 A brief reason (e.g., “to know the fields of OpenMode”, “to see how SearchSelectMode is implemented for MRUMode”).\n\
                 Ask the user to provide the code for those items. The user may paste the code directly or tell you to fetch it via HTTP from http://localhost:{}/<HASH> or http://localhost:{}/file/<FILEPATH>.\n\
                 You can also fetch multiple hashes at once by separating them with '+' or ',', like http://localhost:{}/HASH1+HASH2.\n\
                 Do not guess or stub missing implementations.\n\
                 Do not proceed until you have received all requested code.\n\
                 using as bash \n\
                 cli --skeleton \n\
                 cli --file models/application/modes/open/mod.rs file2.rs file3.rs  # all files in single line\n\
                 cli HASH1+HASH2                          # multiple bodies combined with file:/// tags\n\
                 ===",
                args.daemon_port, args.daemon_port, args.daemon_port
            );

            combined = format!("{}{}{}", header, combined, meta_prompt);

            cache.skeleton = combined.clone();

            let cache_json =
                serde_json::to_string_pretty(&cache).expect("Failed to serialize cache");
            fs::write(&cache_path, cache_json).expect("Failed to write cache file");
        }

        if let Err(e) = fs::write(&args.output, &combined) {
            eprintln!("Error writing to {}: {}", args.output, e);
            std::process::exit(1);
        }

        println!("Written to {} (original files unchanged).", args.output);
    }

    if args.compress || args.resume {
        let state = AppState {
            cache: Arc::new(Mutex::new(cache)),
        };

        let app = Router::new()
            .route("/skeleton", get(get_skeleton))
            .route("/file/*path", get(get_file))
            .route("/:hash", get(get_body))
            .with_state(state);

        let addr = std::net::SocketAddr::from(([127, 0, 0, 1], args.daemon_port));
        println!("Starting body retrieval daemon on http://{}", addr);
        println!("curl http://{}/<HASH>", addr);
        println!("curl http://{}/<HASH1>+<HASH2>", addr);
        println!("curl http://{}/file/src/main.rs", addr);
        println!("curl http://{}/skeleton", addr);

        if let Err(e) = axum::serve(tokio::net::TcpListener::bind(addr).await.unwrap(), app).await {
            eprintln!("Daemon error: {}", e);
        }
    }
}

fn join_into_one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}
