//--+ src/bin/cli.rs
use arboard::Clipboard;
use clap::{Parser, Subcommand};
use dialoguer::Select;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};

static INTERRUPTED: AtomicBool = AtomicBool::new(false);

// ── Terminal helpers ──────────────────────────────────────────────

/// Ensure cursor is visible and formatting is reset.
fn restore_terminal() {
    let _ = std::io::stdout().lock().write_all(b"\x1b[?25h\x1b[0m");
    let _ = std::io::stdout().lock().flush();
}

/// RAII guard that restores the terminal when dropped (safety net).
struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        restore_terminal();
    }
}

fn setup_ctrlc() {
    let _ = ctrlc::set_handler(|| {
        INTERRUPTED.store(true, Ordering::SeqCst);
    });
}

fn is_interrupted() -> bool {
    INTERRUPTED.load(Ordering::SeqCst)
}

fn check_interrupted() {
    if is_interrupted() {
        restore_terminal();
        std::process::exit(130);
    }
}

/// Sleep that can be interrupted by Ctrl+C. Returns `true` if interrupted.
fn interruptible_sleep(duration: std::time::Duration) -> bool {
    let start = std::time::Instant::now();
    let check_interval = std::time::Duration::from_millis(100);
    while start.elapsed() < duration {
        if is_interrupted() {
            return true;
        }
        std::thread::sleep(check_interval.min(duration - start.elapsed()));
    }
    false
}

#[derive(Parser, Debug)]
#[command(name = "concat_rust_cli", about = "CLI for the concat_rust daemon", allow_external_subcommands = true)]
struct Cli {
    // Made optional so running `cli` with no args defaults to `cli use`
    #[command(subcommand)]
    command: Option<Commands>,

    #[arg(long, default_value = "127.0.0.1", global = true)]
    host: String,
    #[arg(long, default_value_t = 7890, global = true)]
    port: u16,
    #[arg(long, default_value_t = 3000, global = true)]
    warn_loc: usize,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Fetch a mix of files and/or hashes in one shot:
    /// `cli src/main.rs src/mod.rs 7a1864469b42 f3cb24d`
    #[command(external_subcommand)]
    Mix(Vec<String>),
    Use {
        repo_id: Option<String>,
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
    /// Retrieve the instruction meta-prompt
    #[command(alias = "p")]
    Prompt {
        /// Write the prompt to this file instead of copying to clipboard.
        /// Parent directories are created automatically.
        #[arg(short = 'o', long, value_name = "PATH")]
        output: Option<String>,
        /// Optional problem description or instruction to append to the prompt
        instruction: Option<String>,
    },
    /// Fetch a mix of files and hashes in one go
    Fetch {
        /// Arguments that can be file paths or hashes (auto-detected)
        args: Vec<String>,
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
fn has_repo_prefix(path: &str) -> bool {
    if let Some(slash_pos) = path.find('/') {
        let first = &path[..slash_pos];
        !first.contains('.') && first != "src"
    } else {
        false
    }
}

fn resolve_path(input: &str, active_repo: Option<&str>) -> String {
    let clean_input = input.strip_prefix('/').unwrap_or(input);
    let with_src = if should_auto_prefix_src(clean_input) {
        format!("src/{}", clean_input)
    } else {
        clean_input.to_string()
    };
    if let Some(repo) = active_repo {
        let repo_prefix = format!("{}/", repo);
        if with_src.starts_with(&repo_prefix) || has_repo_prefix(&with_src) {
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
    check_interrupted();
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
    check_interrupted();
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
    check_interrupted();
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
    check_interrupted();
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

fn fetch_repo_list(base: &str) -> Result<Vec<serde_json::Value>, String> {
    let repos = fetch_json(&format!("{}/repos", base))?;
    repos
        .as_array()
        .cloned()
        .ok_or_else(|| "Unexpected repos format".to_string())
}

/// Prompts the user to select a repo from a list using `dialoguer`.
///
/// Returns:
/// - `Some(Some(id))` if a repo was selected.
/// - `Some(None)` if "Clear active repo" was selected (only if `allow_clear` is true).
/// - `None` if the user cancelled (ESC/q) or an error occurred.
fn prompt_select_repo(base: &str, allow_clear: bool) -> Option<Option<String>> {
    let repos = match fetch_repo_list(base) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("❌ Failed to fetch repos: {}", e);
            return None;
        }
    };

    if repos.is_empty() {
        println!("No repos registered.\n");
        println!("  Add a repo:");
        println!("    cli add-repo myapp /path/to/project");
        println!("    cli add-repo myapp .");
        return None;
    }

    let active = get_active_repo();
    let mut sorted: Vec<_> = repos.iter().collect();
    sorted.sort_by(|a, b| {
        let a_active = active.as_deref() == a["id"].as_str();
        let b_active = active.as_deref() == b["id"].as_str();
        match (a_active, b_active) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a["id"]
                .as_str()
                .unwrap_or("")
                .cmp(b["id"].as_str().unwrap_or("")),
        }
    });

    let repo_count = sorted.len();
    let mut items: Vec<String> = sorted
        .iter()
        .map(|r| {
            let id = r["id"].as_str().unwrap_or("?");
            let branch = r["git_branch"].as_str().unwrap_or("detached");
            let path = r["source_path"].as_str().unwrap_or("?");
            let files = r["file_count"].as_u64().unwrap_or(0);
            let is_active = active.as_deref() == Some(id);

            // Using single-char marker and fixed-width formatters for perfect alignment
            let marker = if is_active { "●" } else { " " };
            format!(
                "{} {:<12} [{:<10}] {:>3} files  {}",
                marker, id, branch, files, path
            )
        })
        .collect();

    if allow_clear {
        items.push("✕ Clear active repo".to_string());
    }

    let default_index = active
        .as_deref()
        .and_then(|a| sorted.iter().position(|r| r["id"].as_str() == Some(a)))
        .unwrap_or(0);

    let result = Select::new()
        .with_prompt("Select a repo")
        .items(&items)
        .default(default_index)
        .interact_opt();

    // ALWAYS restore cursor after dialoguer
    restore_terminal();

    match result {
        Ok(Some(idx)) => {
            if idx < repo_count {
                Some(sorted[idx]["id"].as_str().map(|s| s.to_string()))
            } else if allow_clear && idx == repo_count {
                Some(None) // Clear active repo
            } else {
                None
            }
        }
        Ok(None) => {
            println!("Cancelled.");
            None
        }
        Err(_) => {
            println!("Cancelled or interrupted.");
            None
        }
    }
}

fn cmd_use(repo_id: Option<&str>, base: &str) {
    if let Some(id) = repo_id {
        if id == "none" {
            set_active_repo("none");
            println!("○ no active repo  ·  paths are now fully qualified");
            return;
        }
        let repos = match fetch_repo_list(base) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("❌ Failed to fetch repos: {}", e);
                return;
            }
        };
        if repos.iter().any(|r| r["id"].as_str() == Some(id)) {
            set_active_repo(id);
            println!("✅ Active repo: {}", id);
            println!("  Files will be looked up in repo '{}'", id);
        } else {
            eprintln!("❌ Unknown repo '{}'. Available:", id);
            for r in &repos {
                if let Some(rid) = r["id"].as_str() {
                    eprintln!("  {}", rid);
                }
            }
        }
        return;
    }

    match prompt_select_repo(base, true) {
        Some(Some(chosen)) => {
            set_active_repo(&chosen);
            println!("✅ Active repo: {}", chosen);
            println!("  Files will be looked up in repo '{}'", chosen);
        }
        Some(None) => {
            set_active_repo("none");
            println!("Cleared active repo. Paths must be fully qualified.");
        }
        None => {}
    }
}

fn format_timestamp(ts: u64) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let diff = now.saturating_sub(ts);
    if diff < 60 {
        format!("{}s ago", diff)
    } else if diff < 3600 {
        format!("{}m ago", diff / 60)
    } else if diff < 86400 {
        format!("{}h ago", diff / 3600)
    } else {
        format!("{}d ago", diff / 86400)
    }
}

fn cmd_active(base: &str) {
    const GREEN: &str = "\x1b[32m";
    const DIM: &str = "\x1b[2m";
    const RESET: &str = "\x1b[0m";

    let active = get_active_repo();
    let repos = fetch_json(&format!("{}/repos", base)).ok();

    match active.as_deref() {
        Some("none") => {
            println!("⚪ Active repo: <none>");
            println!();
            println!("  Paths must be fully qualified:");
            println!("  cli file myrepo/src/main.rs");
            println!();
            println!("  Set active: cli use");
        }
        Some(repo_id) => {
            println!("{}🟢 Active repo: {}{}", GREEN, repo_id, RESET);

            if let Some(arr) = repos.as_ref().and_then(|r| r.as_array()) {
                if let Some(repo) = arr.iter().find(|r| r["id"].as_str() == Some(repo_id)) {
                    let path = repo["source_path"].as_str().unwrap_or("?");
                    let branch = repo["git_branch"].as_str().unwrap_or("detached");
                    let files = repo["file_count"].as_u64().unwrap_or(0);
                    let last_sync = repo["last_sync"].as_u64();

                    println!("  ├─ Path:   {}{}{}", DIM, path, RESET);
                    println!("  ├─ Branch: {}{}{}", DIM, branch, RESET);
                    println!("  ├─ Files:  {}{}{}", DIM, files, RESET);
                    if let Some(ts) = last_sync {
                        println!("  └─ Sync:   {}{}{}", DIM, format_timestamp(ts), RESET);
                    }
                }
            }

            println!();
            println!("  Path shorthand:");
            println!(
                "    {}main.rs{}       → {}/src/main.rs",
                DIM, RESET, repo_id
            );
            println!("    {}lib.rs{}        → {}/src/lib.rs", DIM, RESET, repo_id);
            println!(
                "    {}src/main.rs{}   → {}/src/main.rs",
                DIM, RESET, repo_id
            );
            println!("    {}Cargo.toml{}    → {}/Cargo.toml", DIM, RESET, repo_id);
            println!();
            println!("  {}Clear: cli use none{}", DIM, RESET);
        }
        None => {
            println!("⚪ No active repo set");
            println!();
            println!("  Set:  cli use <repo_id>");
            println!("  Pick: cli use");

            if let Some(arr) = repos.as_ref().and_then(|r| r.as_array()) {
                if !arr.is_empty() {
                    println!();
                    println!("  Available:");
                    for r in arr {
                        if let Some(id) = r["id"].as_str() {
                            let path = r["source_path"].as_str().unwrap_or("?");
                            println!("    {}{:12} {}{}", DIM, id, path, RESET);
                        }
                    }
                }
            }
        }
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

    let arr = match repos.as_array() {
        Some(a) if !a.is_empty() => a,
        _ => {
            println!("No repos registered.\n");
            println!("  Add a repo:");
            println!("    cli add-repo myapp /path/to/project");
            println!("    cli add-repo myapp .");
            return;
        }
    };

    let active = get_active_repo();
    let mut total_files = 0u64;

    let mut sorted: Vec<_> = arr.iter().collect();
    sorted.sort_by(|a, b| {
        let a_active = active.as_deref() == a["id"].as_str();
        let b_active = active.as_deref() == b["id"].as_str();
        match (a_active, b_active) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a["id"]
                .as_str()
                .unwrap_or("")
                .cmp(b["id"].as_str().unwrap_or("")),
        }
    });

    const GREEN: &str = "\x1b[32m";
    const DIM: &str = "\x1b[2m";
    const RESET: &str = "\x1b[0m";

    for r in &sorted {
        let id = r["id"].as_str().unwrap_or("?");
        let path = r["source_path"].as_str().unwrap_or("?");
        let files = r["file_count"].as_u64().unwrap_or(0);
        let branch = r["git_branch"].as_str().unwrap_or("detached");
        let last_sync = r["last_sync"].as_u64();

        total_files += files;

        let is_active = active.as_deref() == Some(id);

        if is_active {
            println!(
                "{}→{} {}{}{}  {}[{}]{}",
                GREEN, RESET, GREEN, id, RESET, DIM, branch, RESET
            );
            println!(
                "  {}{}{}  {}({} files, {}){}",
                DIM,
                path,
                RESET,
                DIM,
                files,
                sync_label(last_sync),
                RESET
            );
            println!("  {}↑ active — paths resolve here{}", GREEN, RESET);
        } else {
            println!("  {}  [{}]", id, branch);
            println!(
                "  {}{}{}  {}({} files, {}){}",
                DIM,
                path,
                RESET,
                DIM,
                files,
                sync_label(last_sync),
                RESET
            );
        }
        println!();
    }

    println!("  {} repos, {} files", sorted.len(), total_files);

    match active.as_deref() {
        None | Some("none") => {
            println!("  💡 Set active: cli use");
        }
        Some(id) if !arr.iter().any(|r| r["id"].as_str() == Some(id)) => {
            println!("  ⚠️  Active repo '{}' not found. Use: cli use", id);
        }
        _ => {}
    }
}

fn sync_label(last_sync: Option<u64>) -> String {
    match last_sync {
        Some(ts) => format!("synced {}", format_timestamp(ts)),
        None => "not synced".to_string(),
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

fn cmd_sync(repo: Option<&str>, base: &str) {
    let repo_id = match repo {
        Some(id) => id.to_string(),
        None => {
            let cwd = match std::env::current_dir() {
                Ok(p) => p,
                Err(_) => {
                    eprintln!("❌ Cannot determine current directory");
                    return;
                }
            };
            let cwd_str = cwd.display().to_string();
            let body = serde_json::json!({ "path": cwd_str });

            let mut resolved_id = None;
            if let Ok(resp) = post_json(&format!("{}/resolve", base), &body) {
                if let Ok(data) = serde_json::from_str::<serde_json::Value>(&resp) {
                    if let Some(id) = data["id"].as_str() {
                        resolved_id = Some(id.to_string());
                    }
                }
            }

            if let Some(id) = resolved_id {
                id
            } else {
                eprintln!("⚠️ Not inside a registered repo. Please select one manually.");
                match prompt_select_repo(base, false) {
                    Some(Some(id)) => id,
                    _ => {
                        println!("Sync cancelled.");
                        return;
                    }
                }
            }
        }
    };

    let url = format!("{}/sync/{}", base, repo_id);
    match post_text(&url) {
        Ok(msg) => println!("🔄 {}", msg),
        Err(e) => eprintln!("❌ {}", e),
    }
}

fn cmd_prompt(base: &str, warn_loc: usize, output: Option<&str>, instruction: Option<&str>) {
    let url = format!("{}/meta-prompt", base);
    match fetch_text(&url) {
        Ok(body) => {
            let mut final_content = body;
            if let Some(inst) = instruction {
                let trimmed = inst.trim();
                if !trimmed.is_empty() {
                    final_content = format!("{}{}", trimmed, final_content);
                }
            }

            let loc = final_content.lines().count();

            if let Some(path_str) = output {
                let path = std::path::Path::new(path_str);

                if let Some(parent) = path.parent() {
                    if !parent.as_os_str().is_empty() {
                        if let Err(e) = std::fs::create_dir_all(parent) {
                            eprintln!("❌ Failed to create {}: {}", parent.display(), e);
                            return;
                        }
                    }
                }

                match std::fs::write(path, &final_content) {
                    Ok(_) => println!("✅ Wrote {} LOC to {}", loc, path.display()),
                    Err(e) => eprintln!("❌ Failed to write file: {}", e),
                }
            } else {
                if copy_to_clipboard(&final_content, warn_loc, &[format!("PROMPT [{} LOC]", loc)]) {
                    println!("\n{}", final_content);
                }
            }
        }
        Err(e) => eprintln!("❌ {}", e),
    }
}

fn copy_to_clipboard(content: &str, warn_loc: usize, summaries: &[String]) -> bool {
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
        if interruptible_sleep(std::time::Duration::from_secs(3)) {
            restore_terminal();
            println!("\nInterrupted.");
            std::process::exit(130);
        }
    }
    match Clipboard::new().and_then(|mut cb| cb.set_text(content)) {
        Ok(_) => {
            println!("✅ Copied {} LOC to clipboard:", total_loc);
            for s in summaries {
                println!("  - {}", s);
            }
            true
        }
        Err(e) => {
            eprintln!("⚠️  Clipboard failed: {}. Printing to stdout:\n", e);
            println!("{}", content);
            false
        }
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
                    if interruptible_sleep(std::time::Duration::from_secs(3)) {
                        restore_terminal();
                        println!("\nInterrupted.");
                        std::process::exit(130);
                    }
                }

                match std::fs::write(path, &body) {
                    Ok(_) => println!("✅ Wrote {} LOC to {}", loc, path.display()),
                    Err(e) => eprintln!("❌ Failed to write file: {}", e),
                }
            } else {
                copy_to_clipboard(&body, warn_loc, &[format!("SKELETON [{} LOC]", loc)]);
            }
        }
        Err(e) => eprintln!("❌ {}", e),
    }
}

/// Heuristic: a token is treated as a hash prefix when it is at least 7 chars
/// long and consists solely of ASCII hex digits. This avoids misclassifying
/// ordinary paths like `src/main.rs`, `Cargo.toml`, or `lib.rs`.
fn is_hash(s: &str) -> bool {
    let s = s.trim();
    if s.len() < 7 {
        return false;
    }
    s.chars().all(|c| c.is_ascii_hexdigit())
}

/// Fetch a mix of file paths and hash prefixes in a single invocation.
/// Each token is classified by [`is_hash`]; hashes go to `/{hash}` and files
/// go to `/file/{path}` (after repo/`src/` resolution). Results are
/// concatenated in the order given and copied to the clipboard as one batch.
fn cmd_mix(items: &[String], base: &str, warn_loc: usize) {
    let active = get_active_repo();
    let mut full_content = String::new();
    let mut summaries = Vec::new();

    for item in items {
        for part in item.split(',') {
            check_interrupted();
            let part = part.trim();
            // Skip empty tokens and any flag-like args that external_subcommand
            // may have swept up when the user interleaves `--host`/`--port` etc.
            if part.is_empty() || part.starts_with('-') {
                continue;
            }

            if is_hash(part) {
                let cleaned = strip_hash_prefix(part);
                let url = format!("{}/{}", base, cleaned);
                match fetch_text(&url) {
                    Ok(body) => {
                        let loc = body.lines().count();
                        if !full_content.is_empty() {
                            full_content.push_str("\n\n");
                        }
                        full_content.push_str(&body);
                        summaries.push(format!("Hash: {} [{} LOC]", cleaned, loc));
                    }
                    Err(e) => eprintln!("❌ hash {}: {}", cleaned, e),
                }
            } else {
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
    }

    if !full_content.is_empty() {
        copy_to_clipboard(&full_content, warn_loc, &summaries);
    }
}

fn cmd_file(paths: &[String], base: &str, warn_loc: usize) {
    let active = get_active_repo();
    let mut full_content = String::new();
    let mut summaries = Vec::new();
    for p in paths {
        for part in p.split(',') {
            check_interrupted();
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
        let cleaned = strip_hash_prefix(target);
        let url = format!("{}/info/{}", base, cleaned);
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

fn strip_hash_prefix(hash: &str) -> String {
    let trimmed = hash.trim();
    let lower = trimmed.to_lowercase();
    if lower.starts_with("hash:") || lower.starts_with("hash_") || lower.starts_with("hash-") {
        trimmed[5..].to_string()
    } else {
        trimmed.to_string()
    }
}

/// Detect if an argument is a content hash (vs a file path).
/// Hashes are hex strings (6-64 chars) without path separators or file extensions.
fn is_content_hash(arg: &str) -> bool {
    let stripped = strip_hash_prefix(arg);
    let lower = stripped.to_lowercase();

    // If it looks like a path, it's not a hash
    if arg.contains('/') || arg.contains('\\') || arg.starts_with('.') {
        return false;
    }

    // If it has a file extension, it's not a hash
    if arg.contains('.') {
        return false;
    }

    // Must be 6-64 hex characters
    lower.len() >= 6 && lower.len() <= 64 && lower.chars().all(|c| c.is_ascii_hexdigit())
}

fn cmd_fetch(args: &[String], base: &str, active: Option<&str>, warn_loc: usize) {
    let mut file_args: Vec<String> = Vec::new();
    let mut hash_args: Vec<String> = Vec::new();

    for arg in args {
        let stripped = strip_hash_prefix(arg);
        if is_content_hash(&stripped) {
            hash_args.push(stripped);
        } else {
            file_args.push(arg.clone());
        }
    }

    if file_args.is_empty() && hash_args.is_empty() {
        eprintln!("❌ No arguments provided");
        return;
    }

    let mut full_content = String::new();
    let mut summaries = Vec::new();

    // Fetch files
    for p in &file_args {
        for part in p.split(',') {
            check_interrupted();
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            let resolved = resolve_path(part, active);
            let dp = display_path(&resolved, active);
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

    // Fetch hashes
    if !hash_args.is_empty() {
        let hash_query = hash_args.join("+");
        let url = format!("{}/{}", base, hash_query);
        match fetch_text(&url) {
            Ok(body) => {
                let loc = body.lines().count();
                if !full_content.is_empty() {
                    full_content.push_str("\n\n");
                }
                full_content.push_str(&body);
                summaries.push(format!("Hashes: [{}] [{} LOC]", hash_args.join(", "), loc));
            }
            Err(e) => eprintln!("❌ Hash fetch failed: {}", e),
        }
    }

    if !full_content.is_empty() {
        copy_to_clipboard(&full_content, warn_loc, &summaries);
    }
}

fn cmd_hash(hashes: &[String], base: &str, warn_loc: usize) {
    let cleaned: Vec<String> = hashes.iter().map(|h| strip_hash_prefix(h)).collect();
    let hash_query = cleaned.join("+");
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
                cleaned.join(", "),
                files_str,
                loc
            );
            copy_to_clipboard(&body, warn_loc, &[summary]);
        }
        Err(e) => eprintln!("❌ {}", e),
    }
}

fn main() {
    // Safety net: restore terminal on any exit path
    let _guard = TerminalGuard;

    setup_ctrlc();

    let cli = Cli::parse();
    check_interrupted();

    let base = base_url(&cli);
    match cli.command {
        // If no command is provided, default to interactive `cli use`
        None => cmd_use(None, &base),
        Some(cmd) => match cmd {
            Commands::Mix(items) => cmd_mix(&items, &base, cli.warn_loc),
            Commands::Use { repo_id } => cmd_use(repo_id.as_deref(), &base),
            Commands::Active => cmd_active(&base),
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
            Commands::Prompt {
                output,
                instruction,
            } => cmd_prompt(
                &base,
                cli.warn_loc,
                output.as_deref(),
                instruction.as_deref(),
            ),
            Commands::Fetch { args } => {
                let active = get_active_repo();
                cmd_fetch(&args, &base, active.as_deref(), cli.warn_loc);
            }
        },
    }
}
