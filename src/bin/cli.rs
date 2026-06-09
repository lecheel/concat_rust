use arboard::Clipboard;
use clap::Parser;

/// CLI to retrieve compressed code bodies from the concat_rust daemon
#[derive(Parser, Debug)]
#[command(
    name = "concat_rust_cli",
    about = "Retrieve code bodies by hash or whole files by path, copy to clipboard"
)]
struct Args {
    /// The hash(es) of the code bodies to retrieve (supports multiple in a row)
    #[arg(group = "target")]
    hashes: Vec<String>,

    /// The file path(s) to retrieve entirely (supports multiple, e.g., --file src/main.rs src/utils.rs)
    #[arg(long, group = "target", num_args(1..))]
    file: Option<Vec<String>>,

    /// Retrieve the full skeleton (the compressed output file) from the daemon
    #[arg(short, long, group = "target")]
    skeleton: bool,

    /// The host address of the daemon
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    /// The port of the daemon
    #[arg(long, default_value_t = 7890)]
    port: u16,
}

fn fetch_url(url: &str) -> Result<String, String> {
    match reqwest::blocking::get(url) {
        Ok(resp) => {
            if resp.status().is_success() {
                resp.text()
                    .map_err(|e| format!("Failed to read body: {}", e))
            } else {
                Err(format!("Server returned HTTP {}", resp.status()))
            }
        }
        Err(e) => Err(format!(
            "Failed to connect to daemon at {} (Is it running?)\nError: {}",
            url, e
        )),
    }
}

fn main() {
    let args = Args::parse();
    let mut clipboard_content = String::new();
    let mut summaries = Vec::new();

    // Handle skeleton request
    if args.skeleton {
        let url = format!("http://{}:{}/skeleton", args.host, args.port);
        match fetch_url(&url) {
            Ok(body) => {
                clipboard_content = body;
                summaries.push("SKELETON (full skeleton output)".to_string());
            }
            Err(e) => {
                eprintln!("Error retrieving skeleton: {}", e);
                std::process::exit(1);
            }
        }
    } else if let Some(files) = args.file {
        // Handle whole file request(s)
        for filepath in files {
            let url = format!("http://{}:{}/file/{}", args.host, args.port, filepath);
            match fetch_url(&url) {
                Ok(body) => {
                    if !clipboard_content.is_empty() {
                        clipboard_content.push_str("\n\n");
                    }
                    clipboard_content.push_str(&body);
                    summaries.push(format!("File: {}", filepath));
                }
                Err(e) => {
                    eprintln!("Error retrieving file {}: {}", filepath, e);
                }
            }
        }
    } else {
        // Handle hash(es) request
        for hash in &args.hashes {
            let url = format!("http://{}:{}/{}", args.host, args.port, hash);
            match fetch_url(&url) {
                Ok(body) => {
                    let filename = body
                        .lines()
                        .next()
                        .and_then(|l| l.strip_prefix("// File: "))
                        .unwrap_or("unknown");

                    summaries.push(format!("HASH {} (File: {})", hash, filename));

                    if !clipboard_content.is_empty() {
                        clipboard_content.push_str("\n\n");
                    }
                    clipboard_content.push_str(&body);
                }
                Err(e) => {
                    eprintln!("Error retrieving hash {}: {}", hash, e);
                }
            }
        }
    }

    if clipboard_content.is_empty() {
        eprintln!("No content retrieved.");
        std::process::exit(1);
    }

    // Try to copy to clipboard
    match Clipboard::new().and_then(|mut cb| cb.set_text(&clipboard_content)) {
        Ok(_) => {
            println!("✅ Copied to clipboard:");
            for s in &summaries {
                println!("  - {}", s);
            }
        }
        Err(e) => {
            // Fallback: if clipboard fails (e.g., no X11/Wayland display), print to stdout
            eprintln!("⚠️ Could not copy to clipboard: {}", e);
            eprintln!("Printing to stdout instead:\n");
            println!("{}", clipboard_content);
        }
    }
}
