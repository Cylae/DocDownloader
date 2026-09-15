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

    /// Output PDF file path or directory
    #[arg(short, long, global = true)]
    pub output: Option<PathBuf>,

    /// Number of concurrent page downloads (1-16)
    #[arg(short, long, default_value_t = 4, global = true)]
    pub concurrency: usize,

    /// Force overwrite of existing destination file
    #[arg(short, long, global = true)]
    pub force: bool,

    /// Suppress progress indicators
    #[arg(short, long, global = true)]
    pub quiet: bool,

    /// Enable verbose diagnostic output
    #[arg(short, long, global = true)]
    pub verbose: bool,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Download a publication and reconstruct offline PDF
    Download(DownloadArgs),

    /// Inspect publication metadata without downloading
    Inspect(InspectArgs),

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
pub struct CacheArgs {
    #[command(subcommand)]
    pub action: Option<CacheAction>,
}

#[derive(Subcommand, Debug)]
pub enum CacheAction {
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
