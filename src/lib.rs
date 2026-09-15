pub mod cli;
pub mod core;
pub mod network;
pub mod pdf;
pub mod providers;
pub mod storage;
pub mod web;

pub use core::document::Publication;
pub use core::engine::{DownloadEngine, ProgressListener};
pub use core::error::DocDownloaderError;
pub use core::job::{JobManifest, JobState};
pub use network::client::HttpClient;
pub use providers::{ProviderRegistry, PublicationProvider};
pub use storage::cache::CacheManager;
