//! Runs the actual transfer/auth/filesystem sources without launching a vault or GUI.
extern crate self as tauri;
pub struct AppHandle;
pub struct Paths;
pub trait Manager {
    fn path(&self) -> Paths;
}
impl Manager for AppHandle {
    fn path(&self) -> Paths {
        Paths
    }
}
impl Paths {
    pub fn app_data_dir(&self) -> std::io::Result<std::path::PathBuf> {
        Err(std::io::Error::other(
            "Harness has no application data directory",
        ))
    }
}
#[path = "../../../src-tauri/src/error.rs"]
pub mod error;
pub mod file_exchange;
pub mod sync;
pub mod blob_store {
    pub fn hash_hex(bytes: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        crate::file_exchange::manifest::hex(&Sha256::digest(bytes))
    }
}
