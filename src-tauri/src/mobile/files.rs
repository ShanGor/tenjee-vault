use crate::error::{VaultError, VaultResult};
use std::path::{Path, PathBuf};
use tauri::Manager;

fn root(app: &tauri::AppHandle) -> VaultResult<PathBuf> {
    let path = app.path().app_cache_dir().map_err(|_| VaultError::Validation("Cannot access private cache".into()))?.join("tenjee-transfer");
    std::fs::create_dir_all(&path)?;
    Ok(path)
}

#[tauri::command]
pub async fn mobile_backup_directory_cmd(app: tauri::AppHandle) -> VaultResult<Option<String>> {
    let result = super::call(app, "chooseBackupDirectory", serde_json::json!({})).await?;
    Ok(result["directory"].as_str().map(str::to_string))
}

fn checked(app: &tauri::AppHandle, value: &str) -> VaultResult<PathBuf> {
    let path = Path::new(value).canonicalize()?;
    if !path.starts_with(root(app)?.canonicalize()?) || path == root(app)?.canonicalize()? {
        return Err(VaultError::Validation("Not a private transfer file".into()));
    }
    Ok(path)
}

#[tauri::command]
pub async fn mobile_share_bytes_cmd(app: tauri::AppHandle, data_base64: String, filename: String) -> VaultResult<()> {
    use base64::Engine;
    if data_base64.len() > 64 * 1024 * 1024 { return Err(VaultError::Validation("Attachment exceeds mobile sharing limit (48 MiB)".into())); }
    let bytes = base64::engine::general_purpose::STANDARD.decode(data_base64)
        .map_err(|_| VaultError::Validation("Invalid attachment data".into()))?;
    let directory = mobile_file_workspace_cmd(app.clone())?;
    let destination = crate::portability::file_boundary::SafeDestination::in_selected_directory(Path::new(&directory), &filename)?;
    destination.write_atomic(&bytes, false)?;
    if let Err(error) = mobile_export_file_cmd(app.clone(), destination.path().to_string_lossy().into_owned(), true).await {
        let _ = mobile_release_files_cmd(app, vec![directory]);
        return Err(error);
    }
    Ok(())
}

#[tauri::command]
pub fn mobile_file_workspace_cmd(app: tauri::AppHandle) -> VaultResult<String> {
    let directory = root(&app)?.join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir(&directory)?;
    Ok(directory.to_string_lossy().into_owned())
}

#[tauri::command]
pub async fn mobile_pick_files_cmd(app: tauri::AppHandle, multiple: bool, extensions: Vec<String>) -> VaultResult<Vec<String>> {
    let response = super::call(app, "pickFiles", serde_json::json!({"multiple":multiple,"extensions":extensions})).await?;
    serde_json::from_value(response["paths"].clone()).map_err(|_| VaultError::Validation("Invalid picker response".into()))
}

#[tauri::command]
pub async fn mobile_export_file_cmd(app: tauri::AppHandle, path: String, share: bool) -> VaultResult<bool> {
    let path = checked(&app, &path)?;
    if !path.is_file() { return Err(VaultError::Validation("Export is not a file".into())); }
    let response = super::call(app, "exportFile", serde_json::json!({"path":path,"share":share})).await?;
    Ok(response["completed"].as_bool().unwrap_or(false))
}

#[tauri::command]
pub fn mobile_release_files_cmd(app: tauri::AppHandle, paths: Vec<String>) -> VaultResult<()> {
    for value in paths {
        let path = match checked(&app, &value) { Ok(p) => p, Err(_) => continue };
        if path.is_dir() { std::fs::remove_dir_all(path)?; } else { std::fs::remove_file(path)?; }
    }
    Ok(())
}

#[tauri::command]
pub fn mobile_zip_export_cmd(app: tauri::AppHandle, directory: String) -> VaultResult<String> {
    use std::io::{Read, Write};
    fn add(writer: &mut zip::ZipWriter<std::fs::File>, base: &Path, dir: &Path, count: &mut usize, total: &mut u64) -> VaultResult<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            let metadata = std::fs::symlink_metadata(&path)?;
            if metadata.file_type().is_symlink() { return Err(VaultError::Validation("Export contains a symlink".into())); }
            if metadata.is_dir() { add(writer, base, &path, count, total)?; }
            else if metadata.is_file() {
                *count += 1; *total += metadata.len();
                if *count > 10000 || *total > 1024 * 1024 * 1024 { return Err(VaultError::Validation("Export archive exceeds limits".into())); }
                let name = path.strip_prefix(base).map_err(|_| VaultError::Validation("Invalid export entry".into()))?.to_string_lossy().replace('\\', "/");
                writer.start_file(name, zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated))
                    .map_err(|_| VaultError::Validation("Cannot archive export".into()))?;
                let mut file = std::fs::File::open(path)?;
                let mut buffer = [0; 65536];
                loop { let n = file.read(&mut buffer)?; if n == 0 { break; } writer.write_all(&buffer[..n])?; }
            }
        }
        Ok(())
    }
    let source = checked(&app, &directory)?;
    if !source.is_dir() { return Err(VaultError::Validation("Export is not a directory".into())); }
    let archive = source.with_extension("zip");
    let file = std::fs::OpenOptions::new().write(true).create_new(true).open(&archive)?;
    let result = (|| {
        let mut writer = zip::ZipWriter::new(file);
        add(&mut writer, &source, &source, &mut 0, &mut 0)?;
        writer.finish().map_err(|_| VaultError::Validation("Cannot finish export archive".into()))?.sync_all()?;
        Ok(archive.to_string_lossy().into_owned())
    })();
    if result.is_err() { let _ = std::fs::remove_file(archive); }
    result
}
