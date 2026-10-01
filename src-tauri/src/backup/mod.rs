//! Versioned backup package metadata and backup/restore services.

pub mod manifest;
pub mod reader;
pub mod scheduler;
pub mod snapshot;
pub mod writer;

/// A payload copied while the maintenance gate is held.  The digest is captured from the
/// source and checked again after copying and immediately before ZIP compression, so an
/// external process changing an attachment cannot produce a plausible-looking partial backup.
#[derive(Debug, Clone)]
pub struct FrozenPayload {
    pub relative_path: std::path::PathBuf,
    pub kind: String,
    pub sha256: String,
}
