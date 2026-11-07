//! Local pairing credentials, deliberately outside the backed-up workspace.
use crate::error::{VaultError, VaultResult};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::{io::Write, path::Path, sync::Mutex};
static STORE: Mutex<()> = Mutex::new(());
#[derive(Clone, Serialize, Deserialize)]
pub struct Credential {
    pub id: String,
    pub secret: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct TrustedDevice {
    pub peer: String,
    pub label: String,
    pub credential: Credential,
}
#[derive(Serialize)]
pub struct DeviceSummary {
    pub peer: String,
    pub label: String,
}
fn invalid() -> VaultError {
    VaultError::Validation("Invalid trusted device credential; pair again with a code".into())
}
impl Credential {
    pub fn new() -> Self {
        let mut bytes = zeroize::Zeroizing::new([0u8; 32]);
        rand::rngs::OsRng.fill_bytes(bytes.as_mut());
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            secret: crate::blob_store::hash_hex(bytes.as_ref()),
        }
    }
    pub fn valid(&self) -> bool {
        uuid::Uuid::parse_str(&self.id).is_ok()
            && self.secret.len() == 64
            && self.secret.bytes().all(|b| b.is_ascii_hexdigit())
    }
}
impl Drop for Credential {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.secret.zeroize();
    }
}
fn path(root: &Path) -> VaultResult<std::path::PathBuf> {
    Ok(root
        .parent()
        .ok_or_else(invalid)?
        .join(".tenjee-exchange-trust.json"))
}
fn read(root: &Path) -> VaultResult<Vec<TrustedDevice>> {
    match std::fs::read(path(root)?) {
        Ok(bytes) => serde_json::from_slice(&zeroize::Zeroizing::new(bytes)).map_err(|_| invalid()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(vec![]),
        Err(error) => Err(error.into()),
    }
}
fn write(root: &Path, devices: &[TrustedDevice]) -> VaultResult<()> {
    let path = path(root)?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let temp = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut file = options.open(&temp)?;
        let bytes = zeroize::Zeroizing::new(serde_json::to_vec(devices).map_err(|_| invalid())?);
        file.write_all(&bytes)?;
        file.sync_all()?;
        std::fs::rename(&temp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temp);
    }
    result
}
pub fn lookup(root: &Path, id: &str) -> VaultResult<Option<TrustedDevice>> {
    let _guard = STORE.lock().unwrap();
    Ok(read(root)?
        .into_iter()
        .find(|d| d.credential.id == id && d.credential.valid()))
}
pub fn for_peer(root: &Path, peer: &str) -> VaultResult<Option<TrustedDevice>> {
    let _guard = STORE.lock().unwrap();
    Ok(read(root)?
        .into_iter()
        .find(|d| d.peer == peer && d.credential.valid()))
}
pub fn save(root: &Path, device: TrustedDevice) -> VaultResult<()> {
    if !device.credential.valid() || uuid::Uuid::parse_str(&device.peer).is_err() {
        return Err(invalid());
    }
    let _guard = STORE.lock().unwrap();
    let mut devices = read(root)?;
    devices.retain(|d| d.peer != device.peer && d.credential.id != device.credential.id);
    devices.push(device);
    write(root, &devices)
}
pub fn list(root: &Path) -> VaultResult<Vec<DeviceSummary>> {
    let _guard = STORE.lock().unwrap();
    Ok(read(root)?
        .into_iter()
        .map(|d| DeviceSummary {
            peer: d.peer,
            label: d.label,
        })
        .collect())
}
pub fn revoke(root: &Path, peer: &str) -> VaultResult<()> {
    let _guard = STORE.lock().unwrap();
    let mut devices = read(root)?;
    devices.retain(|d| d.peer != peer);
    write(root, &devices)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn credentials_survive_restart_and_can_be_revoked() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("vault");
        let peer = uuid::Uuid::new_v4().to_string();
        let credential = Credential::new();
        let id = credential.id.clone();
        save(
            &root,
            TrustedDevice {
                peer: peer.clone(),
                label: "Mac".into(),
                credential,
            },
        )
        .unwrap();
        assert_eq!(lookup(&root, &id).unwrap().unwrap().peer, peer);
        assert!(for_peer(&root, &peer).unwrap().is_some());
        assert!(!root.join(".tenjee-exchange-trust.json").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(path(&root).unwrap())
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        revoke(&root, &peer).unwrap();
        assert!(lookup(&root, &id).unwrap().is_none());
    }
}
