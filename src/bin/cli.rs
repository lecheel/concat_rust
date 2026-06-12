// === src/bin/cli.rs ===
// CHANGES: Added has_repo_prefix(), updated resolve_path to detect existing repo prefixes,
// updated cmd_file/cmd_use to show clean display paths, updated resolve message

use arboard::Clipboard;
use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "concat_rust_cli", about = "CLI for the concat_rust daemon")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
    #[arg(long, default_value = "127.0.0.1", global = true)]
    host: String,
    #[arg(long, default_value_t = 7890, global = true)]
    port: u16,
    #[arg(long, default_value_t = 3000, global = true)]
    warn_loc: usize,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Use {
        repo_id: String,
    },
    Active,
    Repos,
    AddRepo {
        id: String,
        source_path: String,
    },
    RemoveRepo {
        id: String,
    },
    Sync {
        repo: Option<String>,
    },
    Catalog {
        #[arg(long)]
        repo: Option<String>,
    },
    Skeleton {
        #[arg(long)]
        repo: Option<String>,
        /// Write the skeleton to this file instead of the clipboard.
        /// Parent directories are created automatically.
        #[arg(short = 'o', long, value_name = "PATH")]
        output: Option<String>,
    },
    File {
        paths: Vec<String>,
    },
    Hash {
        hashes: Vec<String>,
    },
    Info {
        target: String,
        #[arg(long)]
        file: bool,
    },
}

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

fn should_auto_prefix_src(path: &str) -> bool {
    if path.starts_with("src/") || path.contains("/src/") {
        return false;
    }
    let filename = path.rsplit('/').next().unwrap_or(path);
    if ROOT_LEVEL_FILES.contains(&filename) {
        return false;
    }
    if path.contains('/') {
        return true;
    }
    if path.ends_with(".rs") {
        return true;
    }
    false
}

/// Detects if a path already starts with a repo-id prefix (e.g., "grab/src/main.rs").
/// A repo prefix is a first path component that has no dot (not a filename extension) and isn't "src".
fn has_repo_prefix(path: &str) -> bool {
    if let Some(slash_pos) = path.find('/') {
        let first = &path[..slash_pos];
        !first.contains('.') && first != "src"
    } else {
        false
    }
}

fn resolve_path(input: &str, active_repo: Option<&str>) -> String {
    let with_src = if should_auto_prefix_src(input) {
        format!("src/{}", input)
    } else {
        input.to_string()
    };
    if let Some(repo) = active_repo {
        let repo_prefix = format!("{}/", repo);
        if with_src.starts_with(&repo_prefix) || has_repo_prefix(&with_src) {
            // Path already has a repo prefix — use as-is
            with_src
        } else {
            format!("{}{}", repo_prefix, with_src)
        }
    } else {
        with_src
    }
}

/// Returns the display path (without repo prefix) for user-facing messages.
fn display_path<'a>(resolved: &'a str, active_repo: Option<&str>) -> &'a str {
    active_repo
        .and_then(|r| resolved.strip_prefix(&format!("{}/", r)))
        .unwrap_or(resolved)
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

fn cmd_use(repo_id: &str, base: &str) {
    if repo_id == "none" {
        set_active_repo("none");
        println!("Cleared active repo. Paths must be fully qualified.");
        return;
    }
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
            println!("  Files will be looked up in repo '{}'", repo_id);
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
        None => println!("No active repo set. Use: cli use <repo_id>"),
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
    let resolved_path = if source_path == "." {
        std::env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| ".".to_string())
    } else {
        source_path.to_string()
    };
    let body = serde_json::json!({ "id": id, "source_path": resolved_path });
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

fn cmd_skeleton(repo: Option<&str>, base: &str, warn_loc: usize, output: Option<&str>) {
    let mut url = format!("{}/skeleton", base);
    if let Some(r) = repo {
        url = format!("{}?repo={}", url, r);
    }

    match fetch_text(&url) {
        Ok(body) => {
            let loc = body.lines().count();

            if let Some(path_str) = output {
                // --output: persist to disk
                let path = std::path::Path::new(path_str);

                if let Some(parent) = path.parent() {
                    if !parent.as_os_str().is_empty() {
                        if let Err(e) = std::fs::create_dir_all(parent) {
                            eprintln!("❌ Failed to create {}: {}", parent.display(), e);
                            return;
                        }
                    }
                }

                if loc > warn_loc {
                    eprintln!(
                        "⚠️  About to write {} LOC to {} (threshold: {})",
                        loc,
                        path.display(),
                        warn_loc
                    );
                    eprintln!("  Waiting 3s... (Ctrl+C to abort)");
                    std::thread::sleep(std::time::Duration::from_secs(3));
                }

                match std::fs::write(path, &body) {
                    Ok(_) => println!("✅ Wrote {} LOC to {}", loc, path.display()),
                    Err(e) => eprintln!("❌ Failed to write file: {}", e),
                }
            } else {
                // default: clipboard
                copy_to_clipboard(&body, warn_loc, &[format!("SKELETON [{} LOC]", loc)]);
            }
        }
        Err(e) => eprintln!("❌ {}", e),
    }
}

fn cmd_file(paths: &[String], base: &str, warn_loc: usize) {
    let active = get_active_repo();
    let mut full_content = String::new();
    let mut summaries = Vec::new();
    for p in paths {
        for part in p.split(',') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            let resolved = resolve_path(part, active.as_deref());
            let dp = display_path(&resolved, active.as_deref());
            if dp != part {
                eprintln!("  → resolved: {}", dp);
            }
            let url = format!("{}/file/{}", base, resolved);
            match fetch_text(&url) {
                Ok(body) => {
                    let loc = body.lines().count();
                    if !full_content.is_empty() {
                        full_content.push_str("\n\n");
                    }
                    full_content.push_str(&body);
                    summaries.push(format!("File: {} [{} LOC]", dp, loc));
                }
                Err(e) => eprintln!("❌ {}: {}", dp, e),
            }
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
        let dp = display_path(&resolved, active.as_deref());
        if dp != target {
            eprintln!("  → resolved: {}", dp);
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
        Commands::Skeleton { repo, output } => {
            cmd_skeleton(repo.as_deref(), &base, cli.warn_loc, output.as_deref())
        }
        Commands::File { paths } => cmd_file(&paths, &base, cli.warn_loc),
        Commands::Hash { hashes } => cmd_hash(&hashes, &base, cli.warn_loc),
        Commands::Info { target, file } => cmd_info(&target, file, &base),
    }
}
