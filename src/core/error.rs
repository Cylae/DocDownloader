use std::fmt;
use std::path::PathBuf;

/// Structured error categories for DocDownloader.
#[derive(Debug)]
pub enum DocDownloaderError {
    /// URL format or scheme is invalid or not allowed.
    InvalidUrl { url: String, reason: String },

    /// The target URL is not handled by any registered provider.
    UnsupportedProvider { url: String },

    /// The requested publication does not exist on the provider platform.
    PublicationNotFound { id: String, reason: String },

    /// The publication is private, paywalled, or subscriber-restricted.
    /// Access is legitimately denied by the platform.
    AccessRestricted { id: String, reason: String },

    /// The remote platform returned malformed or unexpected metadata.
    InvalidMetadata { id: String, reason: String },

    /// The document manifest could not be retrieved or parsed.
    ManifestUnavailable { id: String, reason: String },

    /// The enumerated page list is empty, non-sequential, or inconsistent.
    PageListInvalid { id: String, reason: String },

    /// A specific page asset could not be located or retrieved.
    PageUnavailable {
        page_index: u32,
        reason: String,
        status: Option<u16>,
    },

    /// A downloaded page asset failed validation (e.g. invalid signature, corrupt image, 0 bytes).
    PageCorrupt { page_index: u32, reason: String },

    /// The server throttled requests (HTTP 429).
    RateLimited {
        retry_after_secs: Option<u64>,
        reason: String,
    },

    /// Network request timed out.
    NetworkTimeout { url: String, elapsed_secs: u64 },

    /// TLS certificate verification or handshake failed.
    TlsError { host: String, reason: String },

    /// SSRF protection blocked a forbidden host or redirect (e.g. loopback, private IP).
    RedirectRejected { url: String, reason: String },

    /// Output destination file already exists and overwrite flag was not provided.
    OutputExists { path: PathBuf },

    /// Filesystem error (permissions, disk full, I/O failure).
    FileSystemError { path: PathBuf, reason: String },

    /// PDF construction failed.
    PdfGenerationFailed { reason: String },

    /// Generated PDF failed independent structural or completeness validation.
    PdfValidationFailed { reason: String },

    /// Operation was cancelled by user (e.g. SIGINT).
    Cancelled,

    /// An internal invariant was violated.
    InternalInvariantViolation { reason: String },
}

impl DocDownloaderError {
    /// Maps the structured error to a process exit code.
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::InvalidUrl { .. } => 1,
            Self::UnsupportedProvider { .. } => 2,
            Self::PublicationNotFound { .. } => 3,
            Self::AccessRestricted { .. } => 3,
            Self::InvalidMetadata { .. }
            | Self::ManifestUnavailable { .. }
            | Self::PageListInvalid { .. } => 4,
            Self::NetworkTimeout { .. } | Self::TlsError { .. } | Self::RateLimited { .. } => 4,
            Self::PageUnavailable { .. } => 4,
            Self::RedirectRejected { .. } => 1,
            Self::PageCorrupt { .. } | Self::PdfValidationFailed { .. } => 5,
            Self::OutputExists { .. } | Self::FileSystemError { .. } => 6,
            Self::PdfGenerationFailed { .. } => 7,
            Self::Cancelled => 130,
            Self::InternalInvariantViolation { .. } => 8,
        }
    }
}

impl fmt::Display for DocDownloaderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidUrl { url, reason } => {
                write!(f, "Invalid URL '{url}': {reason}")
            }
            Self::UnsupportedProvider { url } => {
                write!(f, "No supported provider recognized for URL: {url}")
            }
            Self::PublicationNotFound { id, reason } => {
                write!(f, "Publication '{id}' not found: {reason}")
            }
            Self::AccessRestricted { id, reason } => {
                write!(
                    f,
                    "ACCESS_RESTRICTED: Publication '{id}' is restricted ({reason})"
                )
            }
            Self::InvalidMetadata { id, reason } => {
                write!(f, "Invalid metadata for publication '{id}': {reason}")
            }
            Self::ManifestUnavailable { id, reason } => {
                write!(
                    f,
                    "Failed to load manifest for publication '{id}': {reason}"
                )
            }
            Self::PageListInvalid { id, reason } => {
                write!(f, "Invalid page list for publication '{id}': {reason}")
            }
            Self::PageUnavailable {
                page_index,
                reason,
                status,
            } => {
                if let Some(code) = status {
                    write!(f, "Page {page_index} unavailable (HTTP {code}): {reason}")
                } else {
                    write!(f, "Page {page_index} unavailable: {reason}")
                }
            }
            Self::PageCorrupt { page_index, reason } => {
                write!(f, "Page {page_index} corrupt: {reason}")
            }
            Self::RateLimited {
                retry_after_secs,
                reason,
            } => {
                if let Some(secs) = retry_after_secs {
                    write!(f, "Rate limited by server (retry after {secs}s): {reason}")
                } else {
                    write!(f, "Rate limited by server: {reason}")
                }
            }
            Self::NetworkTimeout { url, elapsed_secs } => {
                write!(
                    f,
                    "Network request to '{url}' timed out after {elapsed_secs}s"
                )
            }
            Self::TlsError { host, reason } => {
                write!(f, "TLS error connecting to '{host}': {reason}")
            }
            Self::RedirectRejected { url, reason } => {
                write!(
                    f,
                    "Security policy rejected URL/redirect to '{url}': {reason}"
                )
            }
            Self::OutputExists { path } => {
                write!(
                    f,
                    "Output file already exists: {} (use --force to overwrite)",
                    path.display()
                )
            }
            Self::FileSystemError { path, reason } => {
                write!(f, "Filesystem error at '{}': {reason}", path.display())
            }
            Self::PdfGenerationFailed { reason } => {
                write!(f, "PDF generation failed: {reason}")
            }
            Self::PdfValidationFailed { reason } => {
                write!(f, "PDF validation failed: {reason}")
            }
            Self::Cancelled => {
                write!(f, "Operation cancelled by user")
            }
            Self::InternalInvariantViolation { reason } => {
                write!(f, "Internal invariant violation: {reason}")
            }
        }
    }
}

impl std::error::Error for DocDownloaderError {}

impl From<std::io::Error> for DocDownloaderError {
    fn from(err: std::io::Error) -> Self {
        Self::FileSystemError {
            path: PathBuf::new(),
            reason: err.to_string(),
        }
    }
}
