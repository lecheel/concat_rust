//--+ src/bin/cli.rs

use arboard::Clipboard;
use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "concat_rust_cli", about = "CLI for the concat_rust daemon")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Daemon host
    #[arg(long, default_value = "127.0.0.1", global = true)]
    host: String,

    /// Daemon port
    #[arg(long, default_value_t = 7890, global = true)]
    port: u16,

    /// Warn if total LOC exceeds this threshold
    #[arg(long, default_value_t = 2000, global = true)]
    warn_loc: usize,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Set active repo (or 'none')
    Use { repo_id: String },

    /// Show active repo
    Active,

    /// List registered repos
    Repos,

    /// Register a new repo
    AddRepo { id: String, source_path: String },

    /// Remove a repo
    RemoveRepo { id: String },

    /// Trigger sync (all or specific repo)
    Sync {
        /// Specific repo ID (syncs all if omitted)
        repo: Option<String>,
    },

    /// Show catalog of all files and LOC
    Catalog {
        #[arg(long)]
        repo: Option<String>,
    },

    /// Fetch skeleton to clipboard
    Skeleton {
        #[arg(long)]
        repo: Option<String>,
    },

    /// Fetch whole file(s) by path
    File {
        /// File paths (e.g., core/src/main.rs api/docker-compose.yml)
        paths: Vec<String>,
    },

    /// Fetch body(ies) by hash
    Hash {
        /// Hash values
        hashes: Vec<String>,
    },

    /// Show LOC metadata without downloading
    Info {
        target: String,
        /// Target is a file path, not a hash
        #[arg(long)]
        file: bool,
    },
}

// ── Helpers ──────────────────────────────────────────────────
// ── Path Resolution ─────────────────────────────────────────

/// Root-level files that should NOT get auto src/ prefix
const ROOT_LEVEL_FILES: &[&str] = &[
    "Cargo.toml",
    "Cargo.lock",
    "docker-compose.yml",
    "docker-compose.yaml",
    "Dockerfile",
    ".env",
    ".env.example",
    "Makefile",
    "README.md",
    "build.rs",
];

/// Determine if `src/` should be auto-prepended to the path.
fn should_auto_prefix_src(path: &str) -> bool {
    // Already has src/ as a component
    if path.starts_with("src/") || path.contains("/src/") {
        return false;
    }

    // Known root-level config files — never prefix
    let filename = path.rsplit('/').next().unwrap_or(path);
    if ROOT_LEVEL_FILES.contains(&filename) {
        return false;
    }

    // Has subdirectories (e.g., daemon/mod.rs) — likely a src path
    if path.contains('/') {
        return true;
    }

    // Single .rs file (e.g., main.rs, lib.rs, sync.rs)
    if path.ends_with(".rs") {
        return true;
    }

    false
}

/// Resolve a user-provided path to a fully-qualified daemon path.
///   daemon/mod.rs  +  repo=grab  →  grab/src/daemon/mod.rs
///   sync.rs        +  repo=grab  →  grab/src/sync.rs
///   Cargo.toml     +  repo=grab  →  grab/Cargo.toml
///   src/main.rs    +  repo=grab  →  grab/src/main.rs
///   grab/src/main.rs  +  no repo →  grab/src/main.rs
fn resolve_path(input: &str, active_repo: Option<&str>) -> String {
    // Step 1: Auto-prepend src/ for source-like paths
    let with_src = if should_auto_prefix_src(input) {
        format!("src/{}", input)
    } else {
        input.to_string()
    };

    // Step 2: Prepend active repo unless path already starts with it
    if let Some(repo) = active_repo {
        let repo_prefix = format!("{}/", repo);
        if with_src.starts_with(&repo_prefix) {
            with_src
        } else {
            format!("{}{}", repo_prefix, with_src)
        }
    } else {
        with_src
    }
}

fn base_url(cli: &Cli) -> String {
    format!("http://{}:{}", cli.host, cli.port)
}

fn fetch_text(url: &str) -> Result<String, String> {
    let resp = reqwest::blocking::get(url).map_err(|e| format!("Connection failed: {}", e))?;
    let status = resp.status();
    let body = resp
        .text()
        .map_err(|e| format!("Failed to read body: {}", e))?;
    if !status.is_success() {
        Err(format!("HTTP {}: {}", status, body))
    } else {
        Ok(body)
    }
}

fn fetch_json(url: &str) -> Result<serde_json::Value, String> {
    let resp = reqwest::blocking::get(url).map_err(|e| format!("Connection failed: {}", e))?;
    let status = resp.status();
    let body = resp
        .text()
        .map_err(|e| format!("Failed to read body: {}", e))?;
    if !status.is_success() {
        return Err(format!("HTTP {}: {}", status, body));
    }
    serde_json::from_str(&body).map_err(|e| {
        let preview = &body[..body.len().min(200)];
        format!("Failed to parse JSON: {} (body: {})", e, preview)
    })
}

fn post_text(url: &str) -> Result<String, String> {
    let client = reqwest::blocking::Client::new();
    let resp = client
        .post(url)
        .send()
        .map_err(|e| format!("Connection failed: {}", e))?;
    let status = resp.status();
    let body = resp
        .text()
        .map_err(|e| format!("Failed to read body: {}", e))?;
    if !status.is_success() {
        Err(format!("HTTP {}: {}", status, body))
    } else {
        Ok(body)
    }
}

fn post_json(url: &str, body: &serde_json::Value) -> Result<String, String> {
    let client = reqwest::blocking::Client::new();
    let resp = client
        .post(url)
        .json(body)
        .send()
        .map_err(|e| format!("Connection failed: {}", e))?;
    let status = resp.status();
    let resp_body = resp
        .text()
        .map_err(|e| format!("Failed to read body: {}", e))?;
    if !status.is_success() {
        Err(format!("HTTP {}: {}", status, resp_body))
    } else {
        Ok(resp_body)
    }
}

fn active_repo_path() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let dir = std::path::PathBuf::from(home).join(".concat_rust");
    let _ = std::fs::create_dir_all(&dir);
    dir.join("active")
}

fn get_active_repo() -> Option<String> {
    std::fs::read_to_string(active_repo_path())
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s != "none")
}

fn set_active_repo(repo: &str) {
    let _ = std::fs::write(active_repo_path(), repo);
}

fn copy_to_clipboard(content: &str, warn_loc: usize, summaries: &[String]) {
    let total_loc = content.lines().count();

    if total_loc > warn_loc {
        eprintln!(
            "⚠️  WARNING: About to copy {} LOC (threshold: {})",
            total_loc, warn_loc
        );
        for s in summaries {
            eprintln!("  - {}", s);
        }
        eprintln!("  Waiting 3s... (Ctrl+C to abort)");
        std::thread::sleep(std::time::Duration::from_secs(3));
    }

    match Clipboard::new().and_then(|mut cb| cb.set_text(content)) {
        Ok(_) => {
            println!("✅ Copied {} LOC to clipboard:", total_loc);
            for s in summaries {
                println!("  - {}", s);
            }
        }
        Err(e) => {
            eprintln!("⚠️  Clipboard failed: {}. Printing to stdout:\n", e);
            println!("{}", content);
        }
    }
}

// ── Command Implementations ──────────────────────────────────

fn cmd_use(repo_id: &str, base: &str) {
    if repo_id == "none" {
        set_active_repo("none");
        println!("Cleared active repo. Paths must be fully qualified.");
        return;
    }

    // Verify repo exists on daemon
    let repos = match fetch_json(&format!("{}/repos", base)) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("❌ Failed to fetch repos: {}", e);
            return;
        }
    };

    if let Some(arr) = repos.as_array() {
        if arr.iter().any(|r| r["id"].as_str() == Some(repo_id)) {
            set_active_repo(repo_id);
            println!("Active repo: {}", repo_id);
            println!(
                "  Paths like 'src/main.rs' will resolve to '{}/src/main.rs'",
                repo_id
            );
        } else {
            eprintln!("❌ Unknown repo '{}'. Available:", repo_id);
            for r in arr {
                if let Some(id) = r["id"].as_str() {
                    eprintln!("  {}", id);
                }
            }
        }
    }
}

fn cmd_active() {
    match get_active_repo() {
        Some(repo) => println!("Active repo: {}", repo),
        None => println!("No active repo set. Paths must be fully qualified (repo/path)."),
    }
}

fn cmd_repos(base: &str) {
    let repos = match fetch_json(&format!("{}/repos", base)) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("❌ {}", e);
            return;
        }
    };

    if let Some(arr) = repos.as_array() {
        if arr.is_empty() {
            println!("No repos registered. Use: cli add-repo <id> <path>");
            return;
        }
        println!("📋 Registered Repos:");
        println!("{}", "─".repeat(60));
        for r in arr {
            let id = r["id"].as_str().unwrap_or("?");
            let path = r["source_path"].as_str().unwrap_or("?");
            let branch = r["git_branch"].as_str().unwrap_or("detached");
            let files = r["file_count"].as_u64().unwrap_or(0);
            let active = if let Some(ar) = get_active_repo() {
                if ar == id {
                    "🟢"
                } else {
                    "⚪"
                }
            } else {
                "⚪"
            };

            println!(
                "{} {:10} {:30} [{}] ({} files)",
                active, id, path, branch, files
            );
        }
    }
}

fn cmd_add_repo(id: &str, source_path: &str, base: &str) {
    let body = serde_json::json!({ "id": id, "source_path": source_path });
    match post_json(&format!("{}/repos", base), &body) {
        Ok(msg) => println!("✅ {}", msg),
        Err(e) => eprintln!("❌ {}", e),
    }
}

fn cmd_remove_repo(id: &str, base: &str) {
    let client = reqwest::blocking::Client::new();
    let url = format!("{}/repos/{}", base, id);
    match client.delete(&url).send() {
        Ok(resp) => {
            let status = resp.status();
            let body = resp.text().unwrap_or_default();
            if status.is_success() {
                println!("✅ {}", body);
            } else {
                eprintln!("❌ HTTP {}: {}", status, body);
            }
        }
        Err(e) => eprintln!("❌ Connection failed: {}", e),
    }
}

fn cmd_sync(_repo: Option<&str>, base: &str) {
    let url = format!("{}/sync", base);
    match post_text(&url) {
        Ok(msg) => println!("🔄 {}", msg),
        Err(e) => eprintln!("❌ {}", e),
    }
}

fn cmd_catalog(_repo: Option<&str>, base: &str) {
    let url = format!("{}/catalog", base);
    match fetch_json(&url) {
        Ok(cat) => {
            let files = cat["files"].as_array().cloned().unwrap_or_default();
            let total_loc = cat["total_loc"].as_u64().unwrap_or(0);
            let total_bodies = cat["total_bodies"].as_u64().unwrap_or(0);

            println!("📊 Catalog ({} LOC, {} bodies)", total_loc, total_bodies);
            println!("{}", "─".repeat(60));

            for f in &files {
                let fp = f["filepath"].as_str().unwrap_or("?");
                let loc = f["loc"].as_u64().unwrap_or(0);
                let num_bodies = f["num_bodies"].as_u64().unwrap_or(0);

                let icon = if loc > 2000 {
                    "🔴"
                } else if loc > 500 {
                    "🟡"
                } else {
                    "🟢"
                };

                println!(
                    "{} {:45} {:>5} LOC  {:>3} bodies",
                    icon, fp, loc, num_bodies
                );

                if let Some(top) = f["top_hashes"].as_array() {
                    for t in top.iter().take(3) {
                        let h = t["hash"].as_str().unwrap_or("?");
                        let l = t["loc"].as_u64().unwrap_or(0);
                        if l > 50 {
                            println!("     └─ {} [{} LOC]", h, l);
                        }
                    }
                }
            }
        }
        Err(e) => eprintln!("❌ {}", e),
    }
}

fn cmd_skeleton(repo: Option<&str>, base: &str, warn_loc: usize) {
    let mut url = format!("{}/skeleton", base);
    if let Some(r) = repo {
        url = format!("{}?repo={}", url, r);
    }

    match fetch_text(&url) {
        Ok(body) => {
            let loc = body.lines().count();
            copy_to_clipboard(&body, warn_loc, &[format!("SKELETON [{} LOC]", loc)]);
        }
        Err(e) => eprintln!("❌ {}", e),
    }
}

fn cmd_file(paths: &[String], base: &str, warn_loc: usize) {
    let active = get_active_repo();
    let mut full_content = String::new();
    let mut summaries = Vec::new();

    for p in paths {
        let resolved = resolve_path(p, active.as_deref());

        if resolved != *p {
            eprintln!("  → resolved: {}", resolved);
        }

        let url = format!("{}/file/{}", base, resolved);
        match fetch_text(&url) {
            Ok(body) => {
                let loc = body.lines().count();
                if !full_content.is_empty() {
                    full_content.push_str("\n\n");
                }
                full_content.push_str(&body);
                summaries.push(format!("File: {} [{} LOC]", resolved, loc));
            }
            Err(e) => eprintln!("❌ {}: {}", resolved, e),
        }
    }

    if !full_content.is_empty() {
        copy_to_clipboard(&full_content, warn_loc, &summaries);
    }
}

fn cmd_info(target: &str, is_file: bool, base: &str) {
    if is_file {
        let active = get_active_repo();
        let resolved = resolve_path(target, active.as_deref());

        if resolved != target {
            eprintln!("  → resolved: {}", resolved);
        }

        let url = format!("{}/file-info/{}", base, resolved);
        match fetch_json(&url) {
            Ok(info) => {
                let fp = info["filepath"].as_str().unwrap_or("?");
                let loc = info["loc"].as_u64().unwrap_or(0);
                let byte_size = info["byte_size"].as_u64().unwrap_or(0);
                let source = info["source"].as_str().unwrap_or("?");

                let icon = if loc > 2000 {
                    "🔴"
                } else if loc > 500 {
                    "🟡"
                } else {
                    "🟢"
                };
                println!(
                    "{} {} [{} LOC | {} bytes | src: {}]",
                    icon, fp, loc, byte_size, source
                );
            }
            Err(e) => eprintln!("❌ {}", e),
        }
    } else {
        let url = format!("{}/info/{}", base, target);
        match fetch_json(&url) {
            Ok(info) => {
                if let Some(arr) = info.as_array() {
                    for i in arr {
                        let h = i["hash"].as_str().unwrap_or("?");
                        let l = i["loc"].as_u64().unwrap_or(0);
                        let b = i["byte_size"].as_u64().unwrap_or(0);
                        let f = i["filepath"].as_str().unwrap_or("?");

                        let warning = if l > 500 {
                            " 🔴 LARGE"
                        } else if l > 100 {
                            " 🟡 MEDIUM"
                        } else {
                            " 🟢 small"
                        };
                        println!("  {} {} LOC ({} bytes) {} [{}]", h, l, b, warning, f);
                    }
                } else {
                    println!("{}", info);
                }
            }
            Err(e) => eprintln!("❌ {}", e),
        }
    }
}

fn cmd_hash(hashes: &[String], base: &str, warn_loc: usize) {
    let hash_query = hashes.join("+");
    let url = format!("{}/{}", base, hash_query);

    match fetch_text(&url) {
        Ok(body) => {
            let loc = body.lines().count();
            let mut found_files = std::collections::BTreeSet::new();
            for line in body.lines() {
                if let Some(path) = line.strip_prefix("//--+ file:///") {
                    found_files.insert(path.to_string());
                }
            }

            let files_str = if found_files.is_empty() {
                "unknown".to_string()
            } else {
                found_files.into_iter().collect::<Vec<_>>().join(", ")
            };

            let summary = format!(
                "Hashes: [{}] (Files: {}) [{} LOC]",
                hashes.join(", "),
                files_str,
                loc
            );

            copy_to_clipboard(&body, warn_loc, &[summary]);
        }
        Err(e) => eprintln!("❌ {}", e),
    }
}

// ── Main ─────────────────────────────────────────────────────

fn main() {
    let cli = Cli::parse();
    let base = base_url(&cli);

    match cli.command {
        Commands::Use { repo_id } => cmd_use(&repo_id, &base),
        Commands::Active => cmd_active(),
        Commands::Repos => cmd_repos(&base),
        Commands::AddRepo { id, source_path } => cmd_add_repo(&id, &source_path, &base),
        Commands::RemoveRepo { id } => cmd_remove_repo(&id, &base),
        Commands::Sync { repo } => cmd_sync(repo.as_deref(), &base),
        Commands::Catalog { repo } => cmd_catalog(repo.as_deref(), &base),
        Commands::Skeleton { repo } => cmd_skeleton(repo.as_deref(), &base, cli.warn_loc),
        Commands::File { paths } => cmd_file(&paths, &base, cli.warn_loc),
        Commands::Hash { hashes } => cmd_hash(&hashes, &base, cli.warn_loc),
        Commands::Info { target, file } => cmd_info(&target, file, &base),
    }
}
