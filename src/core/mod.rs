pub mod diagnostic;
pub mod document;
pub mod engine;
pub mod error;
pub mod job;
pub mod quality;

pub use diagnostic::DiagnosticBundle;
pub use document::{AssetCandidate, AssetType, PageDescriptor, PageGeometry, Publication};
pub use error::DocDownloaderError;
pub use job::{CompletedPageAsset, JobManifest, JobState};
pub use quality::{QualityReport, QualitySegment};
