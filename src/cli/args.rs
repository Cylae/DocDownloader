use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "docdownloader",
    version,
    about = "High-performance document extraction and offline reconstruction engine",
    long_about = "DocDownloader acquires interactive flipbook publications and reconstructs them into high-quality, verified offline PDF documents."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Direct URL to download: `docdownloader <URL>` (shorthand for download)
    #[arg(index = 1)]
    pub url: Option<String>,

    /// Output PDF file path
    #[arg(short, long, global = true)]
    pub output: Option<PathBuf>,

    /// Target directory for saved PDF (uses sanitized publication title)
    #[arg(long, global = true)]
    pub output_dir: Option<PathBuf>,

    /// Number of concurrent page downloads (1-16)
    #[arg(short, long, default_value_t = 4, global = true)]
    pub concurrency: usize,

    /// Request timeout in seconds
    #[arg(long, default_value_t = 45, global = true)]
    pub timeout: u64,

    /// Maximum retry attempts for transient errors
    #[arg(long, default_value_t = 4, global = true)]
    pub retries: u32,

    /// Disable resuming from cached state; fetch all assets fresh
    #[arg(long, global = true)]
    pub no_resume: bool,

    /// Force overwrite of existing destination file
    #[arg(short, long, global = true)]
    pub force: bool,

    /// Suppress progress indicators
    #[arg(short, long, global = true)]
    pub quiet: bool,

    /// Enable verbose diagnostic output
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Optional file path to export a sanitized diagnostic bundle
    #[arg(long, global = true)]
    pub diagnostic: Option<PathBuf>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Download a publication and reconstruct offline PDF
    Download(DownloadArgs),

    /// Inspect publication metadata and quality breakdown without downloading
    Inspect(InspectArgs),

    /// Batch download multiple publications from a URL list file
    Batch(BatchArgs),

    /// Export a sanitized diagnostic bundle for troubleshooting
    Diagnostic(DiagnosticArgs),

    /// Manage local document cache
    Cache(CacheArgs),

    /// Launch local web user interface (bound to 127.0.0.1)
    Serve(ServeArgs),
}

#[derive(Args, Debug)]
pub struct DownloadArgs {
    /// Target publication URL (e.g. Calaméo reader link)
    pub url: String,
}

#[derive(Args, Debug)]
pub struct InspectArgs {
    /// Target publication URL to inspect
    pub url: String,
}

#[derive(Args, Debug)]
pub struct BatchArgs {
    /// Text file containing publication URLs (one per line)
    pub file: PathBuf,
}

#[derive(Args, Debug)]
pub struct DiagnosticArgs {
    /// Target publication URL to diagnose
    pub url: String,

    /// Destination path for diagnostic JSON bundle (defaults to stdout)
    #[arg(short, long)]
    pub output: Option<PathBuf>,
}

#[derive(Args, Debug)]
pub struct CacheArgs {
    #[command(subcommand)]
    pub action: Option<CacheAction>,
}

#[derive(Subcommand, Debug)]
pub enum CacheAction {
    /// Show cache summary status (location, items count, disk usage)
    Status,
    /// List all cached publications
    List,
    /// Clean all cached publications or a specific publication
    Clean {
        /// Optional publication ID to clean
        #[arg(long)]
        id: Option<String>,
        /// Clean all cached items
        #[arg(long)]
        all: bool,
    },
}

#[derive(Args, Debug)]
pub struct ServeArgs {
    /// Host address to bind local web UI (defaults to 127.0.0.1 for security)
    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,

    /// Port for local web UI
    #[arg(short, long, default_value_t = 8080)]
    pub port: u16,
}
