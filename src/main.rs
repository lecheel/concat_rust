//--+ src/main.rs

use clap::Parser;
use grab::stable_hash;

#[derive(Parser, Debug)]
#[command(
    name = "concat_rust",
    about = "Compressed code skeleton daemon with multi-repo sync"
)]
struct Args {
    /// Central directory for synced source mirrors
    #[arg(long, default_value = ".concat_rust_central")]
    central_dir: String,

    /// Port for the daemon
    #[arg(long, default_value_t = 7890)]
    port: u16,

    /// Sync interval in seconds (0 = no periodic sync)
    #[arg(long, default_value_t = 30)]
    sync_interval: u64,

    /// Skip initial sync on startup
    #[arg(long)]
    no_sync: bool,

    /// Skip rustfmt
    #[arg(long)]
    no_format: bool,

    /// rustfmt max width
    #[arg(long, default_value_t = 350)]
    max_width: i32,

    /// Cache file path
    #[arg(long, default_value = "concat_rust.cache")]
    cache: String,
}

fn main() {
    let args = Args::parse();

    println!(" concat_rust v2 starting...");
    println!("  Central dir : {}", args.central_dir);
    println!("  Daemon port : {}", args.port);

    // Quick sanity check that our library is linked and stable_hash works
    let h = stable_hash::stable_hash_body("fn main() {}", "src/main.rs");
    println!("  Sanity hash : {} (should be the same every run)", h);
}
