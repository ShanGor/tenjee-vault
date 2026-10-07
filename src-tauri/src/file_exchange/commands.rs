use super::{
    destination::{self, Identity},
    engine::Intent,
    journal::{self, Journal},
    manifest::*,
    native,
    selection::{self, Selected},
};
use crate::error::VaultResult;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};
use tauri::Manager;

#[derive(Clone)]
struct Source {
    label: String,
    path: String,
    folder: bool,
}
#[derive(Default)]
pub struct FileManager {
    sources: Mutex<BTreeMap<String, Source>>,
    destinations: Mutex<BTreeMap<String, (String, Identity)>>,
    pub preparing: AtomicBool,
    pub cancelled: AtomicBool,
}
impl FileManager {
    pub fn send(&self, root: &Path, batch: &str, acknowledge: bool) -> VaultResult<Intent> {
        let j = Journal::open(root)?;
        let p = j.summary(batch)?;
        if p.files + p.directories == 0 || j.field(batch, "role")? != "send" {
            return Err(invalid("Choose a prepared sender batch"));
        }
        if p.excluded > 0 && !acknowledge {
            return Err(invalid(
                "Review and acknowledge excluded entries before sending",
            ));
        }
        Ok(Intent::Send {
            batch: batch.into(),
        })
    }
    pub fn receive(&self, handle: &str) -> VaultResult<Intent> {
        let map = self.destinations.lock().unwrap();
        let (destination, identity) = map
            .get(handle)
            .ok_or_else(|| invalid("Select a destination before entering exchange"))?;
        Ok(Intent::Receive {
            destination: destination.clone(),
            identity: identity.clone(),
        })
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DestinationChoice {
    handle: String,
    label: String,
    notice: String,
}

#[tauri::command]
pub async fn file_exchange_select_cmd(
    app: tauri::AppHandle,
    folder: bool,
) -> VaultResult<Vec<Selected>> {
    let owner = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        if owner
            .state::<FileManager>()
            .preparing
            .load(Ordering::SeqCst)
        {
            return Err(invalid(
                "Wait for preparation or cancel it before changing selection",
            ));
        }
        #[cfg(target_os = "ios")]
        return Err(invalid("File exchange is unavailable on iOS"));
        #[cfg(target_os = "android")]
        let selections: Vec<(String, String, bool)> = {
            let result = native::call("pickTransferSources", serde_json::json!({"folder":folder}))?;
            let rows: Vec<NativeSource> = serde_json::from_value(result["sources"].clone())
                .map_err(|e| invalid(e.to_string()))?;
            rows.into_iter()
                .map(|r| (r.source, r.name, r.folder))
                .collect()
        };
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        let selections: Vec<(String, String, bool)> = {
            use tauri_plugin_dialog::DialogExt;
            let picker = owner.dialog().file();
            let paths = if folder {
                picker.blocking_pick_folder().map(|p| vec![p])
            } else {
                picker.blocking_pick_files()
            };
            paths
                .unwrap_or_default()
                .into_iter()
                .map(|p| {
                    let path = p.into_path().map_err(|e| invalid(e.to_string()))?;
                    Ok((
                        path.to_string_lossy().into_owned(),
                        path.file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned(),
                        folder,
                    ))
                })
                .collect::<VaultResult<Vec<_>>>()?
        };
        #[cfg(target_os = "ios")]
        let selections: Vec<(String, String, bool)> = Vec::new();
        let manager = owner.state::<FileManager>();
        let mut map = manager.sources.lock().unwrap();
        if map.len() + selections.len() > 1000 {
            return Err(invalid(
                "Select at most 1,000 roots; folders can contain up to 100,000 entries",
            ));
        }
        let mut rows = Vec::new();
        for (path, label, folder) in selections {
            let handle = uuid::Uuid::new_v4().to_string();
            map.insert(
                handle.clone(),
                Source {
                    path,
                    label: label.clone(),
                    folder,
                },
            );
            rows.push(Selected { handle, label });
        }
        Ok(rows)
    })
    .await
    .map_err(|e| invalid(e.to_string()))?
}
#[cfg(target_os = "android")]
#[derive(Deserialize)]
struct NativeSource {
    source: String,
    name: String,
    #[serde(default)]
    folder: bool,
}
#[tauri::command]
pub async fn file_exchange_destination_cmd(
    app: tauri::AppHandle,
) -> VaultResult<Option<DestinationChoice>> {
    let owner = app.clone();
    tauri::async_runtime::spawn_blocking(move||{
        #[cfg(target_os="ios")] return Err(invalid("File exchange is unavailable on iOS"));
        #[cfg(target_os="android")] let (path,label,notice,identity)={
            let result=native::call("pickTransferDestination",serde_json::json!({}))?;
            let Some(uri)=result["uri"].as_str() else{return Ok(None)};
            let identity=native::call("transferDestinationIdentity",serde_json::json!({"uri":uri}))?;
            (uri.to_string(),result["name"].as_str().unwrap_or("Android folder").to_string(),"Android providers may show incomplete documents while saving. Capacity and provider durability can be unknown; keep the app open until saved.".to_string(),decode::<Identity>(&bytes(&identity["identity"])?)?)
        };
        #[cfg(not(any(target_os="android",target_os="ios")))] let (path,label,notice,identity)={
            use tauri_plugin_dialog::DialogExt;
            let Some(selected)=owner.dialog().file().blocking_pick_folder() else{return Ok(None)};
            let selected=selected.into_path().map_err(|e|invalid(e.to_string()))?;
            let dir=destination::absolute_dir(&selected)?;let identity=destination::identity(&dir.dir_metadata()?);
            (selected.to_string_lossy().into_owned(),selected.to_string_lossy().into_owned(),String::new(),identity)
        };
        #[cfg(target_os="ios")] let (path,label,notice,identity):(String,String,String,Identity)=unreachable!();
        let handle=uuid::Uuid::new_v4().to_string();owner.state::<FileManager>().destinations.lock().unwrap().insert(handle.clone(),(path,identity));Ok(Some(DestinationChoice{handle,label,notice}))
    }).await.map_err(|e|invalid(e.to_string()))?
}
#[tauri::command]
pub async fn file_exchange_prepare_cmd(
    app: tauri::AppHandle,
    handles: Vec<String>,
    allow_staging: bool,
) -> VaultResult<Preview> {
    if handles.is_empty() || handles.len() > 1000 {
        return Err(invalid("Select between 1 and 1,000 roots"));
    }
    let m = app.state::<FileManager>();
    if m.preparing.swap(true, Ordering::SeqCst) {
        return Err(invalid("Preparation is already running"));
    }
    m.cancelled.store(false, Ordering::SeqCst);
    let owner = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let manager = owner.state::<FileManager>();
        let map = manager.sources.lock().unwrap();
        let sources = handles
            .iter()
            .map(|h| {
                map.get(h)
                    .cloned()
                    .ok_or_else(|| invalid("Selection handle expired; pick the files again"))
            })
            .collect::<VaultResult<Vec<_>>>()?;
        drop(map);
        let j = Journal::open(&journal::root(&owner)?)?;
        if sources.iter().any(|s| native::provider(&s.path)) {
            prepare_native(&j, &sources, allow_staging, &manager.cancelled)
        } else {
            selection::prepare(
                &j,
                &sources
                    .iter()
                    .map(|s| PathBuf::from(&s.path))
                    .collect::<Vec<_>>(),
                &manager.cancelled,
            )
        }
    })
    .await
    .map_err(|e| invalid(e.to_string()));
    app.state::<FileManager>()
        .preparing
        .store(false, Ordering::SeqCst);
    result?
}
fn prepare_native(
    j: &Journal,
    sources: &[Source],
    allow_staging: bool,
    cancel: &AtomicBool,
) -> VaultResult<Preview> {
    let batch = j.create("send", None, None, "")?;
    let work = (|| {
        let transaction = j.conn.unchecked_transaction()?;
        let mut id = 0;
        let mut live = 0u32;
        let mut used = std::collections::BTreeSet::new();
        j.conn.execute_batch("DROP TABLE IF EXISTS temp.scan_seen; CREATE TEMP TABLE scan_seen(key TEXT PRIMARY KEY,root TEXT NOT NULL); DROP TABLE IF EXISTS temp.scan_roots; CREATE TEMP TABLE scan_roots(key TEXT PRIMARY KEY,label TEXT NOT NULL);")?;
        let mut ordered = sources.to_vec();
        ordered.sort_by_key(|source| !source.folder);
        for source in &ordered {
            let mut label = source.label.clone();
            component(&label)?;
            let mut n = 2;
            while !used.insert(key(&[label.clone()])) {
                label = format!("{} ({n})", source.label);
                component(&label)?;
                n += 1;
            }
            let scan = native::call(
                "beginTransferScan",
                serde_json::json!({"source":source.path,"batch":batch,"allowStaging":allow_staging}),
            )?;
            let mut skipped: Option<Vec<String>> = None;
            let token = scan["token"]
                .as_str()
                .ok_or_else(|| invalid("Invalid native scan"))?
                .to_string();
            loop {
                if cancel.load(Ordering::SeqCst) {
                    let _ = native::call("cancelTransferScan", serde_json::json!({"token":token}));
                    return Err(invalid("Preparation cancelled"));
                }
                let page = native::call("nextTransferScan", serde_json::json!({"token":token}))?;
                let rows: Vec<NativeEntry> = serde_json::from_value(page["entries"].clone())
                    .map_err(|e| invalid(e.to_string()))?;
                if rows.len() > 100 {
                    return Err(invalid("Native manifest page exceeds limit"));
                }
                for row in rows {
                    if skipped
                        .as_ref()
                        .is_some_and(|prefix| row.path.starts_with(prefix))
                    {
                        continue;
                    }
                    skipped = None;
                    if row.kind == Kind::Directory && !row.path.is_empty() {
                        use rusqlite::OptionalExtension;
                        let prior: Option<String> = j
                            .conn
                            .query_row(
                                "SELECT label FROM scan_roots WHERE key=?1",
                                [&row.selection_key],
                                |r| r.get(0),
                            )
                            .optional()?;
                        if let Some(prior) = prior {
                            if prior != label {
                                live -= prune_native_root(j, &batch, &prior)?;
                            }
                        }
                    }
                    if row.kind != Kind::Excluded
                        && j.conn.execute(
                            "INSERT OR IGNORE INTO scan_seen VALUES(?1,?2)",
                            rusqlite::params![row.selection_key, label],
                        )? == 0
                    {
                        if row.kind == Kind::Directory {
                            skipped = Some(row.path.clone());
                        }
                        continue;
                    }
                    if row.kind == Kind::Directory && row.path.is_empty() {
                        j.conn.execute(
                            "INSERT INTO scan_roots VALUES(?1,?2)",
                            rusqlite::params![row.selection_key, label],
                        )?;
                    }
                    if live >= MAX_ENTRIES {
                        return Err(invalid("Selection exceeds 100,000 entries"));
                    }
                    let mut path = vec![label.clone()];
                    path.extend(row.path);
                    let signature = if row.kind == Kind::File {
                        let signature = destination::signature(&native::open_source(&row.source)?)?;
                        if signature.size != row.size {
                            return Err(invalid(
                                "Document changed during preparation; pick or prepare it again",
                            ));
                        }
                        String::from_utf8(bytes(&signature)?).unwrap()
                    } else {
                        String::new()
                    };
                    j.add(
                        &batch,
                        &Entry {
                            id,
                            path,
                            kind: row.kind,
                            size: row.size,
                            reason: row.reason,
                        },
                        &row.source,
                        &signature,
                    )?;
                    id += 1;
                    live += 1;
                }
                if page["done"] == true {
                    break;
                }
            }
        }
        renumber_native_entries(j, &batch)?;
        j.summary(&batch)?;
        j.validate_tree(&batch)?;
        j.set(&batch, "digest", &j.digest(&batch)?)?;
        transaction.commit()?;
        j.summary(&batch)
    })();
    if work.is_err() {
        let _ = native::call("discardTransferBatch", serde_json::json!({"batch":batch}));
        let _ = j.delete(&batch);
    }
    work
}
// A provider can reveal an ancestor after a child root was selected first.
// Replace that earlier root with its location in the ancestor, then compact IDs.
fn prune_native_root(j: &Journal, batch: &str, label: &str) -> VaultResult<u32> {
    let name = key(&[label.into()]);
    let prefix = format!("{name}/");
    let removed = j.conn.execute(
        "DELETE FROM entries WHERE batch=?1 AND (name_key=?2 OR substr(name_key,1,length(?3))=?3)",
        rusqlite::params![batch, name, prefix],
    )?;
    j.conn
        .execute("DELETE FROM scan_seen WHERE root=?1", [label])?;
    j.conn
        .execute("DELETE FROM scan_roots WHERE label=?1", [label])?;
    Ok(removed as u32)
}
fn renumber_native_entries(j: &Journal, batch: &str) -> VaultResult<()> {
    let mut start = 0;
    let mut next = 0;
    loop {
        let page = j.page(batch, start, 100)?;
        if page.is_empty() {
            break;
        }
        start = page.last().unwrap().id + 1;
        for mut entry in page {
            let old = entry.id;
            entry.id = next;
            j.conn.execute(
                "UPDATE entries SET id=?3,wire=?4 WHERE batch=?1 AND id=?2",
                rusqlite::params![batch, old, next, String::from_utf8(bytes(&entry)?).unwrap()],
            )?;
            next += 1;
        }
    }
    Ok(())
}
#[derive(Deserialize)]
struct NativeEntry {
    #[serde(rename = "selectionKey")]
    selection_key: String,
    path: Vec<String>,
    kind: Kind,
    #[serde(with = "wide")]
    size: u64,
    source: String,
    reason: String,
}
#[tauri::command]
pub fn file_exchange_cancel_prepare_cmd(app: tauri::AppHandle) {
    app.state::<FileManager>()
        .cancelled
        .store(true, Ordering::SeqCst);
    let _ = native::call("cancelTransferPreparation", serde_json::json!({}));
}
#[tauri::command]
pub async fn file_exchange_preview_cmd(
    app: tauri::AppHandle,
    batch: String,
    start: u32,
) -> VaultResult<Vec<Entry>> {
    tauri::async_runtime::spawn_blocking(move || {
        Journal::open(&journal::root(&app)?)?.page(&batch, start, 100)
    })
    .await
    .map_err(|e| invalid(e.to_string()))?
}
#[tauri::command]
pub async fn file_exchange_history_cmd(app: tauri::AppHandle) -> VaultResult<Vec<Preview>> {
    tauri::async_runtime::spawn_blocking(move || Journal::open(&journal::root(&app)?)?.list())
        .await
        .map_err(|e| invalid(e.to_string()))?
}
#[tauri::command]
pub async fn file_exchange_discard_cmd(app: tauri::AppHandle, batch: String) -> VaultResult<()> {
    crate::sync::session::exchange_stop_cmd(app.clone()).await?;
    tauri::async_runtime::spawn_blocking(move || discard_batch(&journal::root(&app)?, &batch))
        .await
        .map_err(|e| invalid(e.to_string()))?
}
fn discard_batch(root: &Path, batch: &str) -> VaultResult<()> {
    let j = Journal::open(root)?;
    if j.field(batch, "role")? == "receive" {
        let output = j.field(batch, "output")?;
        if !output.is_empty() {
            let d = super::destination::Destination::reopen(
                Path::new(&output),
                &decode(j.field(batch, "identity")?.as_bytes())?,
            )?;
            let p = j.summary(batch)?;
            for id in 0..p.entries {
                d.discard(id)?;
            }
        }
    }
    if cfg!(target_os = "android") {
        native::call("discardTransferBatch", serde_json::json!({"batch":batch}))?;
    }
    j.delete(batch)
}
#[tauri::command]
pub async fn file_exchange_open_destination_cmd(
    app: tauri::AppHandle,
    batch: String,
) -> VaultResult<()> {
    let j = Journal::open(&journal::root(&app)?)?;
    let output = j.field(&batch, "output")?;
    if output.is_empty() {
        return Err(invalid("This batch has no saved destination"));
    }
    if native::provider(&output) {
        native::call("openTransferDestination", serde_json::json!({"uri":output}))?;
        Ok(())
    } else {
        use tauri_plugin_opener::OpenerExt;
        app.opener()
            .open_path(output, None::<String>)
            .map_err(|e| invalid(e.to_string()))
    }
}

#[tauri::command]
pub async fn file_exchange_enter_cmd(
    app: tauri::AppHandle,
    label: String,
    interface: Option<String>,
    port: Option<u16>,
    role: String,
    batch: Option<String>,
    destination: Option<String>,
    acknowledge_exclusions: bool,
) -> VaultResult<crate::sync::session::Status> {
    if !file_exchange_available_cmd() {
        return Err(invalid(
            "File exchange is in validation; use a development build on Linux, Windows or Android",
        ));
    }
    let root = journal::root(&app)?;
    let intent = match role.as_str() {
        "send" => app.state::<FileManager>().send(
            &root,
            batch
                .as_deref()
                .ok_or_else(|| invalid("Prepare a batch first"))?,
            acknowledge_exclusions,
        )?,
        "receive" => app.state::<FileManager>().receive(
            destination
                .as_deref()
                .ok_or_else(|| invalid("Select a destination first"))?,
        )?,
        _ => return Err(invalid("Choose Send or Receive")),
    };
    crate::sync::session::enter(app, label, interface, port, Some(intent)).await
}

#[tauri::command]
pub fn file_exchange_available_cmd() -> bool {
    cfg!(debug_assertions)
        && cfg!(any(
            target_os = "linux",
            target_os = "windows",
            target_os = "android"
        ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    #[test]
    fn discard_preserves_sources_completed_output_and_unowned_staging_files() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().join("journal");
        let j = Journal::open(&root).unwrap();
        let source = t.path().join("original");
        std::fs::write(&source, b"source").unwrap();
        let batch = j.create("receive", None, None, "").unwrap();
        let d = destination::Destination::create(t.path(), "receive").unwrap();
        let entry = Entry {
            id: 0,
            path: vec!["saved".into()],
            kind: Kind::File,
            size: 3,
            reason: String::new(),
        };
        j.add(&batch, &entry, "", "").unwrap();
        j.set(&batch, "output", &d.location()).unwrap();
        j.set(
            &batch,
            "identity",
            &String::from_utf8(
                bytes(&destination::identity(&d.output.dir_metadata().unwrap())).unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        d.partial(0).unwrap().write_all(b"abc").unwrap();
        d.publish(&entry).unwrap();
        d.partial(0).unwrap().write_all(b"tail").unwrap();
        std::fs::write(d.absolute.join(".tenjee-partials/unowned"), b"keep").unwrap();
        drop(j);
        discard_batch(&root, &batch).unwrap();
        assert_eq!(std::fs::read(d.absolute.join("saved")).unwrap(), b"abc");
        assert_eq!(std::fs::read(source).unwrap(), b"source");
        assert_eq!(
            std::fs::read(d.absolute.join(".tenjee-partials/unowned")).unwrap(),
            b"keep"
        );
        assert!(!d.absolute.join(".tenjee-partials/0.part").exists());
        assert!(!Journal::open(&root).unwrap().exists(&batch).unwrap());
    }
    #[test]
    fn native_child_root_is_replaced_by_its_ancestor_without_duplicate_files() {
        let t = tempfile::tempdir().unwrap();
        let j = Journal::open(t.path()).unwrap();
        let batch = j.create("send", None, None, "").unwrap();
        j.conn.execute_batch("CREATE TEMP TABLE scan_seen(key TEXT PRIMARY KEY,root TEXT);CREATE TEMP TABLE scan_roots(key TEXT PRIMARY KEY,label TEXT);").unwrap();
        let add = |id, path: Vec<&str>, kind| {
            j.add(
                &batch,
                &Entry {
                    id,
                    path: path.into_iter().map(String::from).collect(),
                    kind,
                    size: 0,
                    reason: String::new(),
                },
                "",
                "",
            )
            .unwrap()
        };
        add(0, vec!["child"], Kind::Directory);
        add(1, vec!["child", "file"], Kind::File);
        add(2, vec!["parent"], Kind::Directory);
        j.conn
            .execute("INSERT INTO scan_seen VALUES('child-key','child')", [])
            .unwrap();
        j.conn
            .execute("INSERT INTO scan_roots VALUES('child-key','child')", [])
            .unwrap();
        assert_eq!(prune_native_root(&j, &batch, "child").unwrap(), 2);
        add(3, vec!["parent", "child"], Kind::Directory);
        add(4, vec!["parent", "child", "file"], Kind::File);
        renumber_native_entries(&j, &batch).unwrap();
        j.validate_tree(&batch).unwrap();
        assert_eq!(j.summary(&batch).unwrap().files, 1);
        assert_eq!(
            j.entry(&batch, 2).unwrap().path,
            vec!["parent", "child", "file"]
        );
    }
}
