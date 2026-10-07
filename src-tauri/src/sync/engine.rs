use super::{auth, files, groups::{self,Domain}, storage::{self,Context,Record}, session::Session};
use crate::commands::AppStateInner;
use crate::error::{VaultError,VaultResult};
use serde::{Deserialize,Serialize};
use std::collections::{BTreeMap,BTreeSet};
use std::io::{Read,Write,Seek,SeekFrom};
use rusqlite::OptionalExtension;

#[derive(Clone,Default,Serialize,Deserialize)]
pub struct Summary { pub records: usize, pub deletions: usize, pub conflicts: usize, pub blobs: usize }
#[derive(Clone,Default,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct ExchangeResult { pub applied: usize, pub unchanged: usize, pub conflicts: usize, pub pending_groups: usize, pub pending_blobs: usize, pub pending_local_changes: bool }
#[derive(Clone,Serialize,Deserialize)]
pub struct Hello { pub protocol:String, pub replica:String, pub label:String, pub platform:String, pub schemas:Vec<u64>, pub summary:Summary, pub snapshot:String }
pub struct Store { pub id:String, pub records:Vec<Record>, pub domains:Vec<Domain>, pub active_domains:usize }
pub struct Snapshot { pub stores:Vec<Store>, pub summary:Summary, pub fingerprint:String, pub blobs:tempfile::TempDir, pub pending:usize }

#[derive(Clone,Serialize,Deserialize)]
pub struct Head {
    pub entity:String, pub key:String, pub context:Context, pub deleted:bool, pub hash:String,
    pub page:Option<String>, pub section:Option<String>, pub parent:Option<String>, pub size:Option<u64>,
}
#[derive(Clone,Serialize,Deserialize)]
pub struct GroupMeta { pub id:String, pub context:Context, pub protected:bool, pub key_identity:String, pub eligible:bool, pub alternative:bool }
#[derive(Clone,Serialize,Deserialize)]
pub struct PacketMeta { pub id:String, pub store:String, pub group:Option<GroupMeta> }
#[derive(Clone,Serialize,Deserialize)]
pub struct PacketInventory { pub meta:PacketMeta, pub heads:Vec<Head> }
struct Packet { inventory:PacketInventory, records:Vec<Record> }
#[derive(Serialize,Deserialize)]
struct InventoryStart { packets:usize, heads:usize }
#[derive(Serialize,Deserialize)]
struct PacketStart { meta:PacketMeta, heads:usize }
#[derive(Clone,Serialize,Deserialize)]
struct Request { packet:String, hashes:Vec<String> }

fn canonical_uuid(id:&str)->bool { uuid::Uuid::parse_str(id).is_ok_and(|value|value.to_string()==id) }
fn store_kind(id:&str)->VaultResult<crate::db::migrate::DbKind> {
    use crate::db::migrate::DbKind;
    match id { "meta"=>Ok(DbKind::Meta),"tasks"=>Ok(DbKind::Tasks),"calendar"=>Ok(DbKind::Calendar),id if id.starts_with("space:") && canonical_uuid(&id[6..])=>Ok(DbKind::Space),_=>Err(invalid("Invalid exchange store identity")) }
}
fn head(record:&Record)->VaultResult<Head> {
    let field=|key:&str|record.payload.as_ref().and_then(|p|p[key].as_str()).map(str::to_string);
    let key_id=||serde_json::from_str::<Vec<serde_json::Value>>(&record.key).ok()?.first()?.as_str().map(str::to_string);
    let page=match record.entity.as_str(){"pages"=>key_id(),"page_versions"=>field("page_id"),"attachments"|"taggings"=>field("entity_id"),_=>None};
    Ok(Head{entity:record.entity.clone(),key:record.key.clone(),context:record.context.clone(),deleted:record.deleted,hash:storage::variant_id(record)?,page,section:field("section_id"),parent:field("parent_page_id"),size:record.payload.as_ref().and_then(|p|p["size"].as_u64())})
}
fn packets(snapshot:&Snapshot)->VaultResult<Vec<Packet>> {
    let mut packets=Vec::new();
    for store in &snapshot.stores {
        let mut protected=BTreeSet::new();
        for (index,domain) in store.domains.iter().enumerate().filter(|(_,d)|d.protected) {
            let alternative=index>=store.active_domains;
            let digest=crate::blob_store::hash_hex(&json(domain)?);
            let heads=domain.records.iter().map(head).collect::<VaultResult<Vec<_>>>()?;
            protected.extend(heads.iter().map(|h|h.hash.clone()));
            packets.push(Packet{inventory:PacketInventory{meta:PacketMeta{id:format!("{}:{}:{}",store.id,digest,if alternative{"variant"}else{"active"}),store:store.id.clone(),group:Some(GroupMeta{id:domain.id.clone(),context:domain.context.clone(),protected:true,key_identity:domain.key_identity.clone(),eligible:eligible_domain(domain),alternative})},heads},records:domain.records.clone()});
        }
        let records=store.records.iter().filter(|r|storage::variant_id(r).map(|h|!protected.contains(&h)).unwrap_or(false)).cloned().collect::<Vec<_>>();
        if !records.is_empty() { packets.push(Packet{inventory:PacketInventory{meta:PacketMeta{id:format!("{}:ordinary",store.id),store:store.id.clone(),group:None},heads:records.iter().map(head).collect::<VaultResult<Vec<_>>>()?},records}); }
    }
    Ok(packets)
}
fn send_inventory(stream:&mut impl Write,packets:&[Packet],session:&Session)->VaultResult<()> {
    auth::send(stream,&InventoryStart{packets:packets.len(),heads:packets.iter().map(|p|p.records.len()).sum()})?;
    for packet in packets {
        session.check()?;
        auth::send(stream,&PacketStart{meta:packet.inventory.meta.clone(),heads:packet.records.len()})?;
        for header in &packet.inventory.heads { session.check()?; auth::send(stream,header)?; session.touch(); }
    }
    Ok(())
}
fn receive_inventory(stream:&mut impl Read,session:&Session)->VaultResult<Vec<PacketInventory>> {
    let start:InventoryStart=auth::receive(stream,4096)?;
    if start.packets>100_000 || start.heads>1_000_000 {return Err(invalid("Peer inventory exceeds limits"));}
    let mut packets=Vec::new(); let mut total=0; let mut bytes=0usize; let mut ids=BTreeSet::new();
    for _ in 0..start.packets {
        session.check()?;
        let packet:PacketStart=auth::receive(stream,256*1024)?;
        store_kind(&packet.meta.store)?;
        if packet.meta.id.len()>180 || packet.heads>1_000_000 || !ids.insert(packet.meta.id.clone()) {return Err(invalid("Invalid peer packet identity"));}
        total+=packet.heads;
        if total>start.heads {return Err(invalid("Peer inventory count mismatch"));}
        if let Some(group)=&packet.meta.group {
            if !packet.meta.store.starts_with("space:") || !group.protected || !group.id.starts_with("domain:") || group.id.len()>100 || group.key_identity.len()!=64 || group.context.len()>1024 {return Err(invalid("Invalid protected group declaration"));}
        }
        let mut heads=Vec::new(); let mut hashes=BTreeSet::new();
        for _ in 0..packet.heads {
            let head:Head=auth::receive(stream,256*1024)?;
            if head.entity.len()>80 || head.key.len()>1024 || head.context.is_empty() || head.context.len()>1024 || head.hash.len()!=64 || !head.hash.bytes().all(|b|b.is_ascii_hexdigit()) || !hashes.insert(head.hash.clone()) {return Err(invalid("Invalid peer revision header"));}
            bytes=bytes.saturating_add(json(&head)?.len());
            if bytes>64*1024*1024{return Err(invalid("Peer inventory exceeds 64 MiB"));}
            heads.push(head); session.touch();
        }
        packets.push(PacketInventory{meta:packet.meta,heads});
    }
    if total!=start.heads {return Err(invalid("Peer inventory count mismatch"));}
    Ok(packets)
}

type Conversions=BTreeMap<(String,String),String>;
fn requests(inner:&AppStateInner,local:&[Packet],remote:&[PacketInventory])->VaultResult<(Vec<Request>,usize,Conversions)> {
    let mut requests=Vec::new(); let mut pending=BTreeSet::new();
    let mut local_heads:BTreeMap<(String,String,String),Vec<&Head>>=BTreeMap::new();
    let mut protected_pages=BTreeMap::new();
    for packet in local {
        for head in &packet.inventory.heads {
            local_heads.entry((packet.inventory.meta.store.clone(),head.entity.clone(),head.key.clone())).or_default().push(head);
            if let (Some(group),Some(page))=(&packet.inventory.meta.group,&head.page) {protected_pages.insert((packet.inventory.meta.store.clone(),page.clone()),group);}
        }
    }
    loop {let previous=protected_pages.len();
        for packet in remote.iter().filter(|p|p.meta.group.is_none()) {for header in &packet.heads {
            if header.entity=="pages" {if let (Some(page),Some(parent))=(&header.page,&header.parent) {if let Some(group)=protected_pages.get(&(packet.meta.store.clone(),parent.clone())).copied(){protected_pages.entry((packet.meta.store.clone(),page.clone())).or_insert(group);}}}
        }}if previous==protected_pages.len(){break;}
    }
    // Detect protection/plaintext concurrency from metadata before requesting
    // any competing ordinary body or attachment.
    let mut conversions=Conversions::new();
    for packet in local.iter().filter(|p|p.inventory.meta.group.as_ref().is_some_and(|g|g.eligible && !g.alternative)) {
        let section=packet.records.iter().find(|r|r.entity=="sections" && !r.deleted).and_then(|r|r.payload.as_ref()?.get("id")?.as_str());
        if section.is_some_and(|id|inner.session.is_unlocked(id)) {
            for header in &packet.inventory.heads {if let Some(page)=&header.page{conversions.insert((packet.inventory.meta.store.clone(),page.clone()),packet.inventory.meta.group.as_ref().unwrap().id.clone());}}
        }
    }
    // A new ordinary child inherits the locally protected parent’s boundary.
    loop {let previous=conversions.len();
        for packet in remote.iter().filter(|p|p.meta.group.is_none()) {for header in &packet.heads {
            if header.entity=="pages" {if let (Some(page),Some(parent))=(&header.page,&header.parent) {if let Some(group)=conversions.get(&(packet.meta.store.clone(),parent.clone())).cloned(){conversions.entry((packet.meta.store.clone(),page.clone())).or_insert(group);}}}
        }}if previous==conversions.len(){break;}
    }
    let mut blocked_pages=BTreeSet::new();
    for packet in remote {
        for head in &packet.heads {
            if let Some(page)=&head.page {
                let local_protected=protected_pages.contains_key(&(packet.meta.store.clone(),page.clone()));
                let remote_protected=packet.meta.group.is_some();
                if local_protected != remote_protected {
                    let local=local_heads.get(&(packet.meta.store.clone(),head.entity.clone(),head.key.clone()));
                    if local_protected && !remote_protected && local.is_none() && !conversions.contains_key(&(packet.meta.store.clone(),page.clone())){blocked_pages.insert((packet.meta.store.clone(),page.clone()));}
                    if local.is_some_and(|heads|heads.iter().any(|existing|!storage::dominates(&existing.context,&head.context) && !storage::dominates(&head.context,&existing.context))) {
                        let already_covered=local.is_some_and(|heads|heads.iter().any(|existing|storage::dominates(&existing.context,&head.context)));
                        let can_encrypt=already_covered || (local_protected && !remote_protected && conversions.contains_key(&(packet.meta.store.clone(),page.clone())));
                        let encrypted_successor=!local_protected && remote.iter().filter(|p|p.meta.store==packet.meta.store && p.meta.group.is_some()).flat_map(|p|p.heads.iter()).any(|candidate|candidate.entity==head.entity && candidate.key==head.key && local.is_some_and(|heads|heads.iter().all(|h|storage::dominates(&candidate.context,&h.context))));
                        if !can_encrypt && !encrypted_successor {blocked_pages.insert((packet.meta.store.clone(),page.clone()));}
                    }
                }
            }
        }
    }
    for packet in remote.iter().filter(|p|p.meta.group.is_none()) {for header in &packet.heads {
        if header.entity=="attachments" && header.size.is_some_and(|size|size>MAX_CONVERSION_BLOB) {if let Some(page)=&header.page{if conversions.contains_key(&(packet.meta.store.clone(),page.clone())){blocked_pages.insert((packet.meta.store.clone(),page.clone()));}}}
    }}
    let local_spaces=local.iter().filter(|p|p.inventory.meta.store=="meta").flat_map(|p|p.inventory.heads.iter()).filter(|h|h.entity=="spaces").collect::<Vec<_>>();
    let remote_spaces=remote.iter().filter(|p|p.meta.store=="meta").flat_map(|p|p.heads.iter()).filter(|h|h.entity=="spaces").collect::<Vec<_>>();
    for packet in remote {
        if let Some(space)=packet.meta.store.strip_prefix("space:") {
            let key=serde_json::to_string(&vec![space]).map_err(|_|invalid("Invalid registry header"))?;
            let registry=remote_spaces.iter().filter(|h|h.key==key && !h.deleted).collect::<Vec<_>>();
            if registry.is_empty(){return Err(invalid("Peer space inventory has no active registry identity"));}
            if registry.iter().all(|remote|local_spaces.iter().any(|local|local.key==key && local.deleted && storage::dominates(&local.context,&remote.context))){continue;}
        }
        if packet.meta.group.as_ref().is_some_and(|g|!g.eligible) {pending.insert(packet.meta.id.clone());continue;}
        let blocked=packet.heads.iter().any(|h|h.page.as_ref().is_some_and(|p|blocked_pages.contains(&(packet.meta.store.clone(),p.clone()))));
        if blocked && packet.meta.group.is_some() {pending.insert(packet.meta.id.clone());continue;}
        if packet.meta.group.as_ref().is_some_and(|g|g.alternative) {
            if local.iter().any(|p|p.inventory.meta.id==packet.meta.id){continue;}
            requests.push(Request{packet:packet.meta.id.clone(),hashes:packet.heads.iter().map(|h|h.hash.clone()).collect()});continue;
        }
        let mut hashes=Vec::new();
        for head in &packet.heads {
            if head.page.as_ref().is_some_and(|p|blocked_pages.contains(&(packet.meta.store.clone(),p.clone()))) {pending.insert(format!("{}:{}",packet.meta.store,head.page.as_ref().unwrap()));continue;}
            let existing=local_heads.get(&(packet.meta.store.clone(),head.entity.clone(),head.key.clone()));
            if let Some(existing)=existing {
                if existing.iter().any(|h|h.context==head.context && h.hash!=head.hash) {return Err(invalid("Peer reused a causal revision for different content"));}
                if existing.iter().any(|h|storage::dominates(&h.context,&head.context)) {continue;}
            }
            hashes.push(head.hash.clone());
        }
        if !hashes.is_empty() && packet.meta.group.is_some() {
            // Retaining a protected alternative requires its complete coherent
            // domain, including unchanged wrapping metadata and members.
            hashes=packet.heads.iter().map(|h|h.hash.clone()).collect();
        }
        if !hashes.is_empty() {requests.push(Request{packet:packet.meta.id.clone(),hashes});}
    }
    conversions.retain(|key,_|!blocked_pages.contains(key));
    Ok((requests,pending.len(),conversions))
}

fn invalid(message:&str)->VaultError { VaultError::Validation(message.into()) }
fn json<T:Serialize>(value:&T)->VaultResult<Vec<u8>> { serde_json::to_vec(value).map_err(|_|invalid("Cannot encode exchange inventory")) }

pub fn snapshot(inner:&AppStateInner)->VaultResult<Snapshot> {
    inner.with_workspace(|meta,tasks,calendar,spaces| {
        let mut stores=Vec::new(); let mut summary=Summary::default(); let mut hashes=Vec::new();
        let cache=inner.root.parent().ok_or_else(||invalid("Missing workspace parent"))?.join("tenjee-exchange-snapshots");
        std::fs::create_dir_all(&cache)?;
        let blobs=tempfile::Builder::new().prefix("snapshot-").tempdir_in(&cache)?;
        let mut blob_bytes=0u64; let mut pending=0; let mut entity_bytes=0u64;

        incorporate_space_contexts(meta,spaces)?;
        let mut collect=|id:String,conn:&rusqlite::Connection|->VaultResult<()> {
            let stored_bytes:u64=conn.query_row("SELECT COALESCE((SELECT SUM(length(CAST(payload AS BLOB))+length(context)+length(entity_key)) FROM sync_objects),0)+COALESCE((SELECT SUM(length(CAST(payload AS BLOB))+length(context)+length(entity_key)) FROM sync_conflicts),0)+COALESCE((SELECT SUM(length(CAST(payload AS BLOB))) FROM sync_domain_variants),0)",[],|row|row.get(0))?;
            entity_bytes=entity_bytes.saturating_add(stored_bytes);
            if entity_bytes>MAX_ENTITIES{return Err(invalid("Exchange entity data exceeds the 64 MiB memory budget"));}
            let mut records=storage::inventory(conn)?.into_iter().filter(|r|!scaffolding(r)).collect::<Vec<_>>();
            let conflicts=storage::conflicts(conn)?;
            summary.conflicts+=conflicts.iter().map(|r|(&r.entity,&r.key)).collect::<BTreeSet<_>>().len();
            for record in conflicts.into_iter().filter(|r|!scaffolding(r)) { if !records.contains(&record) {records.push(record);} }
            records.sort_by_key(|r|(r.entity.clone(),r.key.clone(),storage::variant_id(r).unwrap_or_default()));
            summary.records+=records.len(); summary.deletions+=records.iter().filter(|r|r.deleted).count();
            summary.blobs+=records.iter().filter(|r|r.entity=="attachments" && !r.deleted).filter_map(|r|r.payload.as_ref()?.get("hash")?.as_str()).collect::<BTreeSet<_>>().len();
            let mut domains=if id.starts_with("space:"){groups::inventory(conn,&records)?}else{Vec::new()};
            let active_domains=domains.len();
            let mut stmt=conn.prepare("SELECT payload FROM sync_domain_variants ORDER BY group_id,variant_id")?;
            for row in stmt.query_map([],|r|r.get::<_,String>(0))? {
                let domain:Domain=serde_json::from_str(&row?).map_err(|_|invalid("Invalid stored protected variant"))?;
                if !domain.protected { return Err(invalid("Protected conflict contains an ordinary variant")); }
                summary.conflicts+=1;
                domains.push(domain);
            }
            pending+=domains.iter().filter(|d|d.protected && !eligible_domain(d)).count();
            let refs=blob_refs(&records)?.into_iter().chain(domains.iter().flat_map(|d|blob_refs(&d.records).unwrap_or_default())).collect::<BTreeSet<_>>();
            for hash in refs {
                let target=blobs.path().join(&hash);
                if target.exists(){continue;}
                let source=crate::blob_store::blob_path(&blob_directory(inner,&id)?,&hash);
                let metadata=std::fs::symlink_metadata(&source)?;
                if !metadata.is_file() || metadata.len()>MAX_BLOB {return Err(invalid("Snapshot attachment exceeds limits"));}
                blob_bytes=blob_bytes.checked_add(metadata.len()).ok_or_else(||invalid("Snapshot size overflow"))?;
                if blob_bytes>MAX_STAGE {return Err(invalid("Snapshot attachment total exceeds 1 GiB"));}
                std::fs::copy(source,&target)?;
                if hash_file(&target)?!=hash {return Err(invalid("Snapshot attachment hash mismatch"));}
            }
            hashes.push((id.clone(),crate::blob_store::hash_hex(&json(&records)?),crate::blob_store::hash_hex(&json(&domains)?)));
            stores.push(Store{id,records,domains,active_domains}); Ok(())
        };
        collect("meta".into(),meta)?; collect("tasks".into(),tasks)?; collect("calendar".into(),calendar)?;
        let infos=crate::db::registry::list_spaces(meta)?;
        for info in infos { if let Some(conn)=spaces.get(&info.id) { collect(format!("space:{}",info.id),conn)?; } }
        Ok(Snapshot{stores,summary,fingerprint:crate::blob_store::hash_hex(&json(&hashes)?),blobs,pending})
    })
}

pub fn hello(inner:&AppStateInner,label:&str,snapshot:&Snapshot)->VaultResult<Hello> {
    use crate::db::migrate::DbKind;
    Ok(Hello{protocol:auth::PROTOCOL.into(),replica:inner.replica_id.clone(),label:label.into(),platform:std::env::consts::OS.into(),
        schemas:[DbKind::Meta,DbKind::Tasks,DbKind::Calendar,DbKind::Space].iter().map(|k|k.migrations().last().unwrap().version).collect(),summary:snapshot.summary.clone(),snapshot:snapshot.fingerprint.clone()})
}
pub fn validate_hello(local:&Hello,peer:&Hello)->VaultResult<()> {
    if peer.protocol!=local.protocol || peer.schemas!=local.schemas { return Err(invalid("Incompatible exchange schema; update both applications")); }
    if !canonical_uuid(&peer.replica) || peer.replica==local.replica || peer.label.len()>128 || peer.platform.len()>32 || peer.snapshot.len()!=64 || peer.summary.records>1_000_000 { return Err(invalid("Invalid peer exchange identity or declared limits")); }
    Ok(())
}
pub fn approval_binding(connector:&Hello,receiver:&Hello)->VaultResult<String> {
    Ok(crate::blob_store::hash_hex(&json(&(auth::PROTOCOL,"complete-workspace",connector,receiver))?))
}


const MAX_ENTITIES:u64=64*1024*1024;
const MAX_MANIFEST:u64=128*1024*1024;
const MAX_CONVERSION_BLOB:u64=64*1024*1024;
const MAX_BLOB:u64=512*1024*1024;
const MAX_STAGE:u64=1024*1024*1024;
const CHUNK:usize=256*1024;
fn eligible_domain(domain:&Domain)->bool {
    domain.records.iter().filter(|r|r.entity=="pages" && !r.deleted).all(|r|r.payload.as_ref().is_some_and(|p|matches!(p["title_is_encrypted"].as_i64(),Some(0 | 1))))
}
fn blob_directory(inner:&AppStateInner,store:&str)->VaultResult<std::path::PathBuf> {
    match store {"tasks"=>Ok(inner.tasks_files_dir()),id if id.starts_with("space:")=>{store_kind(id)?;Ok(inner.files_dir(&id[6..]))},_=>Err(invalid("Unexpected attachment store"))}
}
fn blob_refs(records:&[Record])->VaultResult<BTreeSet<String>> {
    let mut hashes=BTreeSet::new();
    for record in records.iter().filter(|r|r.entity=="attachments" && !r.deleted) {
        let hash=record.payload.as_ref().and_then(|p|p["hash"].as_str()).ok_or_else(||invalid("Missing attachment hash"))?;
        if !valid_hash(hash){return Err(invalid("Invalid attachment hash"));} hashes.insert(hash.into());
    } Ok(hashes)
}
fn valid_hash(hash:&str)->bool {hash.len()==64 && hash.bytes().all(|b|b.is_ascii_digit() || (b'a'..=b'f').contains(&b))}
fn hash_file(path:&std::path::Path)->VaultResult<String> {
    use sha2::{Digest,Sha256};
    let mut file=std::fs::File::open(path)?;let mut digest=Sha256::new();let mut chunk=[0u8;65536];
    loop {let read=file.read(&mut chunk)?;if read==0 {break;}digest.update(&chunk[..read]);}
    Ok(format!("{:x}",digest.finalize()))
}

#[derive(Serialize,Deserialize)]
struct TransferStart { packets:usize }
#[derive(Clone,Serialize,Deserialize)]
struct IncomingPacket { meta:PacketMeta, records:Vec<Record> }
#[derive(Serialize,Deserialize)]
struct BodyStart { meta:PacketMeta, records:usize, blobs:usize }
#[derive(Serialize,Deserialize)]
struct BlobStart { hash:String, size:u64 }
#[derive(Serialize,Deserialize)]
struct Chunk { offset:u64, data:String }
#[derive(Serialize,Deserialize)]
struct Manifest { version:u32, peer:String, packets:Vec<IncomingPacket> }

fn stage_root(inner:&AppStateInner)->VaultResult<std::path::PathBuf> {
    let root=inner.root.parent().ok_or_else(||invalid("Missing workspace parent"))?.join("tenjee-exchange-staging");
    std::fs::create_dir_all(&root)?;Ok(root)
}
fn receive_requests(stream:&mut impl Read,packets:&[Packet],session:&Session)->VaultResult<Vec<Request>> {
    let count:usize=auth::receive(stream,4096)?;
    if count>packets.len(){return Err(invalid("Invalid requested packet count"));}
    let mut result=Vec::new();let mut ids=BTreeSet::new();
    for _ in 0..count {
        session.check()?;
        let request:Request=auth::receive(stream,16*1024*1024)?;
        let packet=packets.iter().find(|p|p.inventory.meta.id==request.packet).ok_or_else(||invalid("Unknown requested packet"))?;
        let available=packet.inventory.heads.iter().map(|h|h.hash.as_str()).collect::<BTreeSet<_>>();
        if !ids.insert(request.packet.clone()) || request.hashes.is_empty() || request.hashes.len()>available.len() || request.hashes.iter().any(|h|!available.contains(h.as_str())) || request.hashes.iter().collect::<BTreeSet<_>>().len()!=request.hashes.len(){return Err(invalid("Invalid revision request"));}
        if let Some(group)=&packet.inventory.meta.group {if !group.eligible || request.hashes.len()!=available.len(){return Err(invalid("Protected domain must transfer completely"));}}
        result.push(request);
    }Ok(result)
}
fn send_requests(stream:&mut impl Write,requests:&[Request])->VaultResult<()> {auth::send(stream,&requests.len())?;for request in requests{auth::send(stream,request)?;}Ok(())}
fn send_bodies(stream:&mut (impl Read+Write),packets:&[Packet],requests:&[Request],snapshot:&Snapshot,session:&Session)->VaultResult<()> {
    use base64::Engine as _;
    auth::send(stream,&TransferStart{packets:requests.len()})?;
    for request in requests {
        session.check()?;
        let packet=packets.iter().find(|p|p.inventory.meta.id==request.packet).ok_or_else(||invalid("Missing approved packet"))?;
        let records=packet.records.iter().filter(|r|storage::variant_id(r).is_ok_and(|h|request.hashes.contains(&h))).cloned().collect::<Vec<_>>();
        let blobs=blob_refs(&records)?;
        auth::send(stream,&BodyStart{meta:packet.inventory.meta.clone(),records:records.len(),blobs:blobs.len()})?;
        for record in &records {session.check()?;auth::send(stream,record)?;session.progress_record();}
        for hash in blobs {
            let mut file=std::fs::File::open(snapshot.blobs.path().join(&hash))?;
            let size=file.metadata()?.len();auth::send(stream,&BlobStart{hash,size})?;
            let mut offset:u64=auth::receive(stream,4096)?;
            if offset>size {return Err(invalid("Invalid attachment resume offset"));}
            file.seek(SeekFrom::Start(offset))?;let mut bytes=vec![0u8;CHUNK];
            while offset<size {session.check()?;let read=file.read(&mut bytes)?;if read==0{return Err(invalid("Snapshot blob was truncated"));}
                auth::send(stream,&Chunk{offset,data:base64::engine::general_purpose::STANDARD.encode(&bytes[..read])})?;offset+=read as u64;session.progress_bytes(read as u64);}
        }
    }Ok(())
}
fn receive_bodies(inner:&AppStateInner,stream:&mut (impl Read+Write),inventory:&[PacketInventory],requests:&[Request],peer:&str,session:&Session,conversions:&Conversions)->VaultResult<tempfile::TempDir> {
    use base64::Engine as _;
    let root=stage_root(inner)?;
    let cache=blob_cache(inner)?;
    let start:TransferStart=auth::receive(stream,4096)?;
    if start.packets!=requests.len(){return Err(invalid("Transfer does not match approved requests"));}
    let stage=tempfile::Builder::new().prefix("incoming-").tempdir_in(&root)?;
    std::fs::create_dir(stage.path().join("blobs"))?;
    let allocations=inner.with_workspace(|meta,tasks,calendar,spaces|{
        let mut counters=BTreeMap::new();
        let mut stmt=meta.prepare("SELECT origin,sequence FROM sync_origin_limits")?;
        for row in stmt.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,u64>(1)?)))? {let (origin,sequence)=row?;counters.insert(origin,sequence);}
        for conn in [&*meta,&*tasks,&*calendar].into_iter().chain(spaces.values()) {let (origin,sequence):(String,u64)=conn.query_row("SELECT origin,sequence FROM sync_state",[],|r|Ok((r.get(0)?,r.get(1)?)))?;counters.insert(origin,sequence);}Ok(counters)
    })?;
    let mut total=0u64;let mut entity_bytes=0u64;let mut packets=Vec::new();let mut seen=BTreeSet::new();
    for _ in 0..start.packets {
        session.check()?;
        let body:BodyStart=auth::receive(stream,256*1024)?;
        if let Some(group)=&body.meta.group {for (origin,counter) in &group.context {
            if origin.starts_with(&format!("{}-",inner.replica_id)) && allocations.get(origin).copied().unwrap_or(0)<*counter{return Err(invalid("Peer claimed an unallocated local group revision"));}
        }}
        let request=requests.iter().find(|r|r.packet==body.meta.id).ok_or_else(||invalid("Unexpected payload packet"))?;
        let headers=inventory.iter().find(|p|p.meta.id==body.meta.id).ok_or_else(||invalid("Missing payload inventory"))?;
        if json(&body.meta)?!=json(&headers.meta)? || body.records!=request.hashes.len() || body.blobs>body.records || !seen.insert(body.meta.id.clone()){return Err(invalid("Payload declaration mismatch"));}
        let mut validation=rusqlite::Connection::open_in_memory()?;
        crate::db::connection::configure(&validation)?;
        crate::db::migrate::run_migrations(&mut validation,store_kind(&body.meta.store)?.migrations())?;
        let mut records=Vec::new();let mut hashes=BTreeSet::new();
        for _ in 0..body.records {
            session.check()?;
            let record:Record=auth::receive(stream,16*1024*1024)?;
            storage::validate(&validation,&record)?;
            for (origin,counter) in &record.context {
                if origin.starts_with(&format!("{}-",inner.replica_id)) && allocations.get(origin).copied().unwrap_or(0)<*counter{return Err(invalid("Peer claimed an unallocated local revision"));}
            }
            let bytes=json(&record)?;total+=bytes.len() as u64;entity_bytes+=bytes.len() as u64;
            if entity_bytes>MAX_ENTITIES{return Err(invalid("Exchange entity data exceeds the 64 MiB memory budget"));}
            if total>MAX_STAGE || fs2::available_space(stage.path())?<bytes.len() as u64+32*1024*1024 {return Err(invalid("Exchange staging storage limit reached"));}
            let header=head(&record)?;
            if !request.hashes.contains(&header.hash) || !hashes.insert(header.hash.clone()) || !headers.heads.iter().any(|h|json(h).ok()==json(&header).ok()){return Err(invalid("Payload revision differs from inventory"));}
            records.push(record);session.progress_record();
        }
        let record_groups=if body.meta.group.is_none(){conversion_record_groups(inner,&body.meta.store,&records,conversions)?}else{BTreeMap::new()};
        let refs=blob_refs(&records)?;
        if refs.len()!=body.blobs{return Err(invalid("Attachment declaration mismatch"));}
        let mut received=BTreeSet::new();let mut converted_blobs=BTreeMap::new();
        for _ in 0..body.blobs {
            session.check()?;
            let blob:BlobStart=auth::receive(stream,4096)?;
            if !refs.contains(&blob.hash) || !received.insert(blob.hash.clone()) || blob.size>MAX_BLOB{return Err(invalid("Invalid attachment declaration"));}
            total=total.checked_add(blob.size).ok_or_else(||invalid("Exchange size overflow"))?;
            if total>MAX_STAGE || fs2::available_space(stage.path())?<blob.size+32*1024*1024 {return Err(invalid("Insufficient storage for exchange"));}
            let conversion_groups=records.iter().filter(|r|r.entity=="attachments" && !r.deleted && r.payload.as_ref().is_some_and(|p|p["hash"]==blob.hash)).filter_map(|r|record_groups.get(&(r.entity.clone(),r.key.clone())).cloned()).collect::<BTreeSet<_>>();
            if !conversion_groups.is_empty() {
                if blob.size>MAX_CONVERSION_BLOB{return Err(invalid("Protection conflict attachment exceeds the 64 MiB in-memory conversion limit"));}
                auth::send(stream,&0u64)?;
                let mut bytes=zeroize::Zeroizing::new(Vec::with_capacity(blob.size as usize));let mut offset=0;
                while offset<blob.size {
                    session.check()?;let chunk:Chunk=auth::receive(stream,CHUNK*2)?;
                    let part=zeroize::Zeroizing::new(base64::engine::general_purpose::STANDARD.decode(&chunk.data).map_err(|_|invalid("Invalid conflict attachment encoding"))?);
                    if chunk.offset!=offset || part.is_empty() || part.len()>CHUNK || offset+part.len() as u64>blob.size{return Err(invalid("Invalid conflict attachment range"));}
                    bytes.extend_from_slice(&part);offset+=part.len() as u64;session.progress_bytes(part.len() as u64);
                }
                if crate::blob_store::hash_hex(&bytes)!=blob.hash{return Err(invalid("Conflict attachment integrity check failed"));}
                for group in &conversion_groups {
                    let key=conversion_key(inner,&body.meta.store,group)?;
                    let encrypted=crate::crypto::cipher::seal(&bytes,key.as_ref())?;
                    let hash=write_staged_blob(stage.path(),&encrypted)?;converted_blobs.insert((blob.hash.clone(),group.clone()),hash);
                }
                // An unrelated ordinary attachment can intentionally share these
                // bytes; retain its ordinary copy only when it is requested too.
                let ordinary=records.iter().filter(|r|r.entity=="attachments" && !r.deleted && r.payload.as_ref().is_some_and(|p|p["hash"]==blob.hash)).any(|r|!record_groups.contains_key(&(r.entity.clone(),r.key.clone())));
                if ordinary {write_staged_blob(stage.path(),&bytes)?;}
                continue;
            }
            let cached=cache.join(&blob.hash);
            let temporary=cache.join(format!("{}.part",blob.hash));
            if cached.is_file() && (std::fs::metadata(&cached)?.len()!=blob.size || hash_file(&cached)?!=blob.hash) {std::fs::remove_file(&cached)?;}
            if cached.is_file() {
                auth::send(stream,&blob.size)?;
            } else {
                if temporary.is_file() && std::fs::metadata(&temporary)?.len()>blob.size {std::fs::remove_file(&temporary)?;}
                let existing=if temporary.is_file(){std::fs::metadata(&temporary)?.len()}else{0};
                if directory_size(&cache)?.saturating_add(blob.size-existing)>MAX_STAGE {return Err(invalid("Attachment resume cache exceeds 1 GiB; clear pending exchange files or free storage"));}
                let mut file=std::fs::OpenOptions::new().write(true).create(true).truncate(false).open(&temporary)?;
                let mut offset=existing;file.seek(SeekFrom::Start(offset))?;auth::send(stream,&offset)?;
                while offset<blob.size {
                    session.check()?;
                    let chunk:Chunk=auth::receive(stream,CHUNK*2)?;
                    let bytes=base64::engine::general_purpose::STANDARD.decode(&chunk.data).map_err(|_|invalid("Invalid attachment chunk"))?;
                    if chunk.offset!=offset || bytes.is_empty() || bytes.len()>CHUNK || offset+bytes.len() as u64>blob.size{return Err(invalid("Invalid attachment chunk range"));}
                    file.write_all(&bytes)?;file.sync_data()?;offset+=bytes.len() as u64;session.progress_bytes(bytes.len() as u64);
                }
                file.sync_all()?;drop(file);
                if hash_file(&temporary)?!=blob.hash {std::fs::remove_file(temporary)?;return Err(invalid("Attachment integrity check failed"));}
                std::fs::rename(temporary,&cached)?;
            }
            let target=stage.path().join("blobs").join(&blob.hash);
            if !target.exists(){std::fs::copy(&cached,&target)?;files::sync_file(&target)?;}
            if hash_file(&target)?!=blob.hash{return Err(invalid("Staged attachment integrity check failed"));}
        }
        if let Some(group)=&body.meta.group {
            let domain=Domain{id:group.id.clone(),context:group.context.clone(),protected:true,key_identity:group.key_identity.clone(),records:records.clone()};
            validate_domain(&domain)?;
        }
        if body.meta.group.is_none() && !record_groups.is_empty() {
            packets.extend(convert_plaintext_conflicts(inner,&body.meta.store,records,&record_groups,&converted_blobs,stage.path())?);
        }else{packets.push(IncomingPacket{meta:body.meta,records});}
    }
    let manifest=Manifest{version:1,peer:peer.into(),packets};
    let bytes=json(&manifest)?;
    if bytes.len() as u64>MAX_MANIFEST{return Err(invalid("Exchange recovery metadata exceeds the memory budget"));}
    if directory_size(&root)?.saturating_add(bytes.len() as u64)>MAX_STAGE {return Err(invalid("Pending exchange staging exceeds 1 GiB"));}
    let mut file=std::fs::OpenOptions::new().write(true).create_new(true).open(stage.path().join("manifest.json"))?;
    file.write_all(&bytes)?;file.sync_all()?;
    Ok(stage)
}
fn validate_domain(domain:&Domain)->VaultResult<()> {
    if !eligible_domain(domain){return Err(invalid("Protected payload has an invalid title format"));}
    if domain.context.is_empty() || domain.context.len()>1024 || domain.context.iter().any(|(origin,counter)|origin.is_empty() || origin.len()>80 || !origin.bytes().all(|b|b.is_ascii_alphanumeric() || b==b'-') || *counter==0 || *counter>i64::MAX as u64) || domain.records.iter().any(|r|!storage::dominates(&domain.context,&r.context)){return Err(invalid("Invalid protected causal context"));}
    let root=domain.id.strip_prefix("domain:").ok_or_else(||invalid("Invalid domain ID"))?;
    let wrapper=domain.records.iter().filter(|r|r.entity=="sections" && !r.deleted).collect::<Vec<_>>();
    if wrapper.len()!=1{return Err(invalid("Protected domain must contain exactly one wrapping record"));}
    let payload=wrapper[0].payload.as_ref().ok_or_else(||invalid("Missing wrapping record"))?;
    let section=payload["id"].as_str().ok_or_else(||invalid("Missing section identity"))?;
    if payload["root_page_id"].as_str().unwrap_or(section)!=root || payload["is_encrypted"].as_i64()!=Some(1){return Err(invalid("Protected domain identity mismatch"));}
    let key=serde_json::json!([payload["is_encrypted"],payload["kdf_salt"],payload["kdf_params"],payload["verifier"],payload["wrapped_dsk"]]);
    if crate::blob_store::hash_hex(&json(&key)?)!=domain.key_identity{return Err(invalid("Protected wrapping identity mismatch"));}
    let pages=domain.records.iter().filter(|r|r.entity=="pages" && !r.deleted).filter_map(|r|r.payload.as_ref()?.get("id")?.as_str()).collect::<BTreeSet<_>>();
    for record in domain.records.iter().filter(|r|!r.deleted) {
        let payload=record.payload.as_ref().ok_or_else(||invalid("Missing domain member payload"))?;
        let valid=match record.entity.as_str(){
            "sections"=>true,"pages"|"section_templates"=>payload["section_id"]==section,
            "page_versions"=>payload["page_id"].as_str().is_some_and(|id|pages.contains(id)),
            "attachments"|"taggings"=>payload["entity_type"]=="page" && payload["entity_id"].as_str().is_some_and(|id|pages.contains(id)),
            _=>false,
        };
        if !valid{return Err(invalid("Protected packet contains a foreign dependency"));}
        let ciphertext=|value:&serde_json::Value,binary:bool|->VaultResult<()> {
            use base64::Engine as _;
            let bytes=if binary {let hex=value.get("$blob").and_then(|v|v.as_str()).ok_or_else(||invalid("Missing ciphertext bytes"))?;
                if hex.len()%2!=0 || !hex.bytes().all(|b|b.is_ascii_hexdigit()){return Err(invalid("Invalid ciphertext encoding"));}
                hex.as_bytes().chunks(2).map(|p|u8::from_str_radix(std::str::from_utf8(p).unwrap(),16).unwrap()).collect::<Vec<_>>()
            }else{base64::engine::general_purpose::STANDARD.decode(value.as_str().ok_or_else(||invalid("Missing ciphertext text"))?).map_err(|_|invalid("Invalid ciphertext encoding"))?};
            if bytes.len()<29 || bytes[0]!=crate::crypto::cipher::FORMAT_V1{return Err(invalid("Unsupported protected ciphertext format"));}Ok(())
        };
        match record.entity.as_str(){
            "pages"=>{
                if payload["title_is_encrypted"].as_i64()==Some(1) {ciphertext(&payload["title"],false)?;}
                else if !payload["title"].is_string() {return Err(invalid("Invalid page title"));}
                if payload["content"]!=""{ciphertext(&payload["content"],false)?;}
            },
            "page_versions"=>{if payload["content"]!=""{ciphertext(&payload["content"],false)?;}},
            "section_templates"=>{ciphertext(&payload["name_ciphertext"],true)?;ciphertext(&payload["content_ciphertext"],true)?;},
            "sections"=>{ciphertext(&payload["wrapped_dsk"],true)?;ciphertext(&payload["verifier"],true)?;},_=>{}
        }

    }Ok(())
}

fn apply_packet(conn:&mut rusqlite::Connection,packet:&IncomingPacket)->VaultResult<storage::ApplyResult> {
    use rusqlite::params;
    for record in &packet.records {storage::validate(conn,record)?;}
    let tx=conn.savepoint()?;
    tx.execute_batch("PRAGMA defer_foreign_keys=ON; UPDATE sync_state SET importing=1;")?;
    let result=if let Some(group)=&packet.meta.group {
        let incoming=Domain{id:group.id.clone(),context:group.context.clone(),protected:true,key_identity:group.key_identity.clone(),records:packet.records.clone()};
        validate_domain(&incoming)?;
        let current=groups::inventory(&tx,&storage::inventory(&tx)?)?.into_iter().find(|d|d.id==incoming.id);
        let variants:usize=tx.query_row("SELECT COUNT(*) FROM sync_domain_variants WHERE group_id=?1",[&incoming.id],|r|r.get(0))?;
        if group.alternative {
            if let Some(current)=&current {
                if current.protected {
                    groups::retain_variant(&tx,current)?;groups::retain_variant(&tx,&incoming)?;
                    tx.execute("UPDATE sync_state SET importing=0",[])?;tx.commit()?;
                    return Ok(storage::ApplyResult{conflicts:1,..Default::default()});
                }
            }
            // The first encrypted successor may replace an ordinary source;
            // every ordinary page must be causally covered before materializing.
            for record in incoming.records.iter().filter(|r|r.entity=="pages") {
                if let Some(existing)=storage::get(&tx,&record.entity,&record.key)? {
                    if existing.payload.as_ref().is_some_and(|p|p["title_is_encrypted"].as_i64()==Some(0)) && !storage::dominates(&record.context,&existing.context){return Err(invalid("Plaintext protection race remains pending"));}
                }
            }
        }
        match current {
            Some(current) if storage::dominates(&current.context,&incoming.context)=>storage::ApplyResult{unchanged:packet.records.len(),..Default::default()},
            Some(current) if variants>0 || (!storage::dominates(&incoming.context,&current.context) && current.key_identity!=incoming.key_identity)=>{
                if !current.protected || !eligible_domain(&current){return Err(invalid("Protection transition requires local resolution before receiving a body"));}
                groups::retain_variant(&tx,&current)?;groups::retain_variant(&tx,&incoming)?;
                storage::ApplyResult{conflicts:1,..Default::default()}
            }
            _=>{
                let result=apply_rows_or_retain(&tx,&packet.records)?;
                groups::save_context(&tx,&incoming.id,&incoming.context)?;
                for record in &packet.records {
                    tx.execute("INSERT INTO sync_members(entity,entity_key,group_id) VALUES(?1,?2,?3) ON CONFLICT(entity,entity_key) DO UPDATE SET group_id=excluded.group_id",params![record.entity,record.key,incoming.id])?;
                }
                if group.alternative {groups::retain_variant(&tx,&incoming)?;}
                result
            }
        }
    } else {
        if packet.meta.store.starts_with("space:") {
            if packet.records.iter().any(|r|!r.deleted && r.payload.as_ref().is_some_and(|p|(r.entity=="sections" && p["is_encrypted"].as_i64()==Some(1)) || (r.entity=="pages" && p["title_is_encrypted"].as_i64()==Some(1)) || r.entity=="section_templates")){return Err(invalid("Protected members require a coherent protected packet"));}
            let domains=groups::inventory(&tx,&storage::inventory(&tx)?)?;
            let protected=domains.iter().filter(|d|d.protected).flat_map(|d|d.records.iter().map(|r|(&r.entity,&r.key))).collect::<BTreeSet<_>>();
            if packet.records.iter().any(|r|protected.contains(&(&r.entity,&r.key)) && !r.deleted){return Err(invalid("Ordinary payload cannot replace a protected member"));}
            for record in packet.records.iter().filter(|r|!r.deleted) {
                if record.entity=="pages" && record.payload.as_ref().is_some_and(|p|domains.iter().any(|d|d.protected && d.records.iter().any(|r|r.entity=="sections" && r.payload.as_ref().is_some_and(|section|section["id"]==p["section_id"])))) {return Err(invalid("Ordinary page cannot enter a protected domain"));}
            }
        }
        apply_rows_or_retain(&tx,&packet.records)?
    };
    tx.execute("UPDATE sync_state SET importing=0",[])?;
    tx.commit()?;
    groups::refresh_members(conn)?;
    Ok(result)
}

/// Ready manifests are a durable coordinator. Per-store receipts share the
/// application transaction through an owning-store savepoint.
fn apply_manifest(inner:&AppStateInner,directory:&std::path::Path,session:Option<&Session>)->VaultResult<ExchangeResult> {
    let path=directory.join("ready.json");
    if std::fs::metadata(&path)?.len()>MAX_MANIFEST{return Err(invalid("Stored exchange manifest exceeds limits"));}
    let bytes=std::fs::read(&path)?;
    if bytes.len() as u64>MAX_MANIFEST{return Err(invalid("Stored exchange manifest exceeds limits"));}
    let manifest:Manifest=serde_json::from_slice(&bytes).map_err(|_|invalid("Invalid exchange recovery manifest"))?;
    if manifest.version!=1 || !canonical_uuid(&manifest.peer){return Err(invalid("Unsupported exchange recovery manifest"));}
    let batch=crate::blob_store::hash_hex(&bytes);
    inner.with_workspace(|meta,tasks,calendar,spaces| {
        incorporate_space_contexts(meta,spaces)?;
        let mut result=ExchangeResult::default();
        let mut by_store:BTreeMap<String,Vec<IncomingPacket>>=BTreeMap::new();
        for packet in manifest.packets {store_kind(&packet.meta.store)?;by_store.entry(packet.meta.store.clone()).or_default().push(packet);}
        // Space databases are migrated before registry materialization; paths
        // are constructed from validated UUIDs and never read from the peer.
        for id in by_store.keys().filter(|id|id.starts_with("space:")) {
            if !spaces.contains_key(&id[6..]) {
                let mut conn=crate::db::connection::open_db(&inner.root.join(format!("spaces/{}.db",&id[6..])))?;
                crate::db::migrate::run_migrations(&mut conn,store_kind(id)?.migrations())?;
                storage::set_origin(&conn,&format!("{}-{}",inner.replica_id,&id[6..]))?;
                spaces.insert(id[6..].into(),conn);
            }
        }
        let mut order=by_store.keys().cloned().collect::<Vec<_>>();order.sort_by_key(|id|match id.as_str(){"meta"=>0,"tasks"=>1,"calendar"=>2,_=>3});
        for id in order {
            if session.is_some_and(|s|s.check().is_err()){result.pending_groups+=by_store.get(&id).map(|p|p.len()).unwrap_or(0);continue;}
            let conn:&mut rusqlite::Connection=match id.as_str(){"meta"=>meta,"tasks"=>tasks,"calendar"=>calendar,_=>spaces.get_mut(&id[6..]).unwrap()};
            conn.execute_batch("CREATE TABLE IF NOT EXISTS sync_batches(batch_id TEXT PRIMARY KEY, result TEXT NOT NULL);")?;
            let prior:Option<String>=conn.query_row("SELECT result FROM sync_batches WHERE batch_id=?1",[&batch],|r|r.get(0)).optional()?;
            if let Some(prior)=prior {
                let prior:storage::ApplyResult=serde_json::from_str(&prior).map_err(|_|invalid("Invalid stored exchange receipt"))?;
                result.applied+=prior.applied;result.conflicts+=prior.conflicts;result.unchanged+=prior.unchanged;continue;
            }
            let mut packets=by_store.remove(&id).unwrap();
            packets.sort_by_key(|p|std::cmp::Reverse(p.meta.group.as_ref().map(|g|g.context.values().map(|v|*v as u128).sum::<u128>()).unwrap_or(0)));
            for packet in &packets {
                for record in &packet.records {storage::validate(conn,record)?;}
                if let Some(group)=&packet.meta.group {validate_domain(&Domain{id:group.id.clone(),context:group.context.clone(),protected:true,key_identity:group.key_identity.clone(),records:packet.records.clone()})?;}
                for hash in blob_refs(&packet.records)? {
                    let source=directory.join("blobs").join(&hash);
                    if hash_file(&source)?!=hash{return Err(invalid("Recovery blob integrity check failed"));}
                    let target=crate::blob_store::blob_path(&blob_directory(inner,&id)?,&hash);
                    if !target.exists(){
                        let parent=target.parent().ok_or_else(||invalid("Missing attachment directory"))?;std::fs::create_dir_all(parent)?;
                        let temporary=parent.join(format!("{}.exchange-{}",hash,uuid::Uuid::new_v4()));
                        std::fs::copy(&source,&temporary)?;files::sync_file(&temporary)?;
                        std::fs::rename(temporary,&target)?;
                    } else if hash_file(&target)?!=hash {return Err(invalid("Existing attachment integrity check failed"));}
                }
            }
            let old_blobs=blob_refs(&storage::inventory(conn)?)?;
            // Savepoint keeps every packet in this store and its receipt atomic.
            conn.execute_batch("SAVEPOINT exchange_store;")?;
            let applied=(||{
                let mut combined=storage::ApplyResult::default();
                // Ordinary records in one transaction preserve cross-entity dependencies.
                let ordinary=packets.iter().filter(|p|p.meta.group.is_none()).flat_map(|p|p.records.clone()).collect::<Vec<_>>();
                if !ordinary.is_empty(){let packet=IncomingPacket{meta:PacketMeta{id:String::new(),store:id.clone(),group:None},records:ordinary};let r=apply_packet(conn,&packet)?;combined.applied+=r.applied;combined.conflicts+=r.conflicts;combined.unchanged+=r.unchanged;}
                for packet in packets.iter().filter(|p|p.meta.group.is_some()) {
                    // Clear current and incoming keys before changing any protected metadata.
                    // Replacement may remove the old section or introduce a new section ID.
                    for section in inner.session.unlocked_section_ids() {
                        let belongs:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM sections WHERE id=?1)",[&section],|row|row.get(0))?;
                        if belongs {inner.session.lock(&section);let _=crate::search::drop_unlocked_index(conn,&section);}
                    }
                    for record in packet.records.iter().filter(|r|r.entity=="sections" && !r.deleted) {
                        if let Some(section)=record.payload.as_ref().and_then(|p|p["id"].as_str()){inner.session.lock(section);let _=crate::search::drop_unlocked_index(conn,section);}
                    }
                    let r=apply_packet(conn,packet)?;combined.applied+=r.applied;combined.conflicts+=r.conflicts;combined.unchanged+=r.unchanged;
                }
                let broken:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM pragma_foreign_key_check)",[],|r|r.get(0))?;
                if broken{return Err(invalid("Exchange dependency references are incomplete"));}
                conn.execute("INSERT INTO sync_batches(batch_id,result) VALUES(?1,?2)",rusqlite::params![batch,serde_json::to_string(&combined).map_err(|_|invalid("Cannot encode exchange receipt"))?])?;
                Ok::<_,VaultError>(combined)
            })();
            match applied {
                Ok(r)=>{if let Err(error)=conn.execute_batch("RELEASE exchange_store;"){let _=conn.execute_batch("ROLLBACK TO exchange_store; RELEASE exchange_store;");return Err(error.into());}result.applied+=r.applied;result.conflicts+=r.conflicts;result.unchanged+=r.unchanged;
                    if id=="tasks" || id.starts_with("space:"){let files=blob_directory(inner,&id)?;for hash in &old_blobs{crate::blob_store::remove_if_unref(&files,conn,hash)?;}}}
                Err(_)=>{conn.execute_batch("ROLLBACK TO exchange_store; RELEASE exchange_store;")?;result.pending_groups+=packets.len();}
            }
        }
        Ok(result)
    })
}

pub fn recover(inner:&AppStateInner)->VaultResult<ExchangeResult> {
    let mut total=ExchangeResult::default();
    let root=stage_root(inner)?;
    for entry in std::fs::read_dir(root)? {
        let entry=entry?;if !entry.file_type()?.is_dir(){continue;}
        let path=entry.path();
        if !path.join("ready.json").is_file(){std::fs::remove_dir_all(path)?;continue;}
        if manifest_has_protection(&path)? {let cache=blob_cache(inner)?;std::fs::remove_dir_all(cache)?;}
        let result=apply_manifest(inner,&path,None)?;
        total.applied+=result.applied;total.conflicts+=result.conflicts;total.pending_groups+=result.pending_groups;
        if result.pending_groups==0 {std::fs::remove_dir_all(path)?;}
    }Ok(total)
}

/// Protecting/moving content cannot leave a pending plaintext exchange file.
/// The caller first stops and joins the active exchange worker.
pub fn protection_barrier(inner:&AppStateInner)->VaultResult<()> {
    let result=recover(inner)?;
    if result.pending_groups>0{return Err(invalid("Resolve pending exchange dependencies before changing protection"));}
    let cache=blob_cache(inner)?;std::fs::remove_dir_all(cache)?;
    cleanup_snapshots(inner)?;
    Ok(())
}

pub fn exchange(inner:&AppStateInner,stream:&mut(impl Read+Write),snapshot:&Snapshot,peer:&str,receiver:bool,session:&Session)->VaultResult<ExchangeResult> {
    let local=packets(snapshot)?;
    let remote=if receiver {let remote=receive_inventory(stream,session)?;send_inventory(stream,&local,session)?;remote}
        else {send_inventory(stream,&local,session)?;receive_inventory(stream,session)?};
    let (mut wanted,mut pending,conversions)=requests(inner,&local,&remote)?;
    recover(inner)?;
    let pending_plaintext=pending_plaintext_pages(inner)?;
    let removed=wanted.iter().filter(|request|remote.iter().any(|packet|packet.meta.id==request.packet && packet.meta.group.is_some() && packet.heads.iter().any(|h|h.page.as_ref().is_some_and(|page|pending_plaintext.contains(&(packet.meta.store.clone(),page.clone())))))).count();
    wanted.retain(|request|remote.iter().all(|packet|packet.meta.id!=request.packet || packet.meta.group.is_none() || packet.heads.iter().all(|h|h.page.as_ref().is_none_or(|page|!pending_plaintext.contains(&(packet.meta.store.clone(),page.clone()))))));
    pending+=removed;
    let requested=if receiver {let requested=receive_requests(stream,&local,session)?;send_requests(stream,&wanted)?;requested}
        else {send_requests(stream,&wanted)?;receive_requests(stream,&local,session)?};
    session.progress_total(wanted.iter().map(|r|r.hashes.len()).sum::<usize>()+requested.iter().map(|r|r.hashes.len()).sum::<usize>());
    let staged=if receiver {let stage=receive_bodies(inner,stream,&remote,&wanted,peer,session,&conversions)?;send_bodies(stream,&local,&requested,snapshot,session)?;stage}
        else {send_bodies(stream,&local,&requested,snapshot,session)?;receive_bodies(inner,stream,&remote,&wanted,peer,session,&conversions)?};
    session.check()?;
    let pending_local_changes=snapshot_without_blobs(inner)?.fingerprint!=snapshot.fingerprint;
    let directory=staged.keep();
    files::publish_ready(&directory)?;
    if manifest_has_protection(&directory)? {let cache=blob_cache(inner)?;std::fs::remove_dir_all(cache)?;}
    let mut result=apply_manifest(inner,&directory,Some(session))?;
    if result.pending_groups==0 {std::fs::remove_dir_all(&directory)?;}
    result.pending_groups+=pending+snapshot.pending;
    result.pending_local_changes=pending_local_changes;
    session.result(result.clone());
    session.check()?;
    // Acknowledgment is sent only after committed store receipts. No transport
    // authorization survives the session; another run always inventories heads.
    if receiver {let _:ExchangeResult=auth::receive(stream,32768)?;auth::send(stream,&result)?;}else{auth::send(stream,&result)?;let _:ExchangeResult=auth::receive(stream,32768)?;}
    Ok(result)
}
fn snapshot_without_blobs(inner:&AppStateInner)->VaultResult<Snapshot> {snapshot(inner)}

fn blob_cache(inner:&AppStateInner)->VaultResult<std::path::PathBuf> {
    let root=inner.root.parent().ok_or_else(||invalid("Missing workspace parent"))?.join("tenjee-exchange-blobs");
    std::fs::create_dir_all(&root)?;
    #[cfg(unix)] {use std::os::unix::fs::PermissionsExt;std::fs::set_permissions(&root,std::fs::Permissions::from_mode(0o700))?;}
    Ok(root)
}
fn directory_size(path:&std::path::Path)->VaultResult<u64> {
    let mut total=0u64;
    for entry in std::fs::read_dir(path)? {let entry=entry?;let meta=entry.metadata()?;
        if meta.is_dir(){total=total.saturating_add(directory_size(&entry.path())?);}else if meta.is_file(){total=total.saturating_add(meta.len());}else{return Err(invalid("Unsupported staging file type"));}
    }Ok(total)
}
fn manifest_has_protection(path:&std::path::Path)->VaultResult<bool> {
    let manifest:Manifest=serde_json::from_slice(&std::fs::read(path.join("ready.json"))?).map_err(|_|invalid("Invalid recovery manifest"))?;
    Ok(manifest.packets.iter().any(|p|p.meta.group.is_some()))
}

fn scaffolding(record:&Record)->bool {
    (record.entity=="notebooks" && record.key=="[\"__page_storage__\"]") || (record.entity=="sections" && record.key=="[\"__plain_pages__\"]")
}
/// A space deletion must observe every child revision. Folding child knowledge
/// into the registry head also makes a remote edit race recoverable as a conflict.
pub fn incorporate_space_contexts(meta:&mut rusqlite::Connection,spaces:&std::collections::HashMap<String,rusqlite::Connection>)->VaultResult<()> {
    let tx=meta.savepoint()?;
    tx.execute_batch("CREATE TABLE IF NOT EXISTS sync_origin_limits(origin TEXT PRIMARY KEY,sequence INTEGER NOT NULL);")?;
    for info in crate::db::registry::list_spaces(&tx)? {
        let Some(conn)=spaces.get(&info.id) else {continue;};
        let key=serde_json::to_string(&vec![&info.id]).map_err(|_|invalid("Invalid registry identity"))?;
        let Some(mut head)=storage::get(&tx,"spaces",&key)? else {continue;};
        let (origin,sequence):(String,u64)=conn.query_row("SELECT origin,sequence FROM sync_state",[],|r|Ok((r.get(0)?,r.get(1)?)))?;
        tx.execute("INSERT INTO sync_origin_limits(origin,sequence) VALUES(?1,?2) ON CONFLICT(origin) DO UPDATE SET sequence=max(sequence,excluded.sequence)",rusqlite::params![origin,sequence])?;
        let mut context=head.context.clone();
        let child_context=|child:&Context|child.iter().map(|(origin,counter)|(format!("s{}",crate::blob_store::hash_hex(format!("{}:{origin}",info.id).as_bytes())),*counter)).collect::<Context>();
        for record in storage::inventory(conn)?.into_iter().chain(storage::conflicts(conn)?).filter(|r|!scaffolding(r)) {context=storage::union(&context,&child_context(&record.context));}
        for domain in groups::inventory(conn,&storage::inventory(conn)?)?.into_iter().filter(|d|d.protected) {context=storage::union(&context,&child_context(&domain.context));}
        if context!=head.context {
            let old=storage::variant_id(&head)?;
            let retained:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM sync_conflicts WHERE entity='spaces' AND entity_key=?1 AND variant_id=?2)",rusqlite::params![key,old],|r|r.get(0))?;
            head.context=context;storage::save_head(&tx,&head)?;
            if retained{tx.execute("DELETE FROM sync_conflicts WHERE entity='spaces' AND entity_key=?1 AND variant_id=?2",rusqlite::params![key,old])?;storage::retain_conflict(&tx,&head)?;}
        }
    }tx.commit()?;Ok(())
}

/// A move/delete whose dependent rows are concurrent must preserve alternatives
/// rather than leave an invalid materialized hierarchy or an unresolvable file.
fn apply_rows_or_retain(conn:&rusqlite::Connection,records:&[Record])->VaultResult<storage::ApplyResult> {
    conn.execute_batch("SAVEPOINT incoming_rows;")?;
    let applied=(|| {
        let result=storage::apply_records(conn,records)?;
        let invalid:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM pragma_foreign_key_check)",[],|r|r.get(0))?;
        if invalid{return Err(invalid_error("Exchange dependency references are incomplete"));}Ok::<_,VaultError>(result)
    })();
    match applied {
        Ok(result)=>{conn.execute_batch("RELEASE incoming_rows;")?;Ok(result)},
        Err(_)=>{
            conn.execute_batch("ROLLBACK TO incoming_rows; RELEASE incoming_rows;")?;
            let mut result=storage::ApplyResult::default();
            for record in records {
                if let Some(current)=storage::get(conn,&record.entity,&record.key)? {
                    if storage::dominates(&current.context,&record.context){result.unchanged+=1;continue;}
                    storage::retain_conflict(conn,&current)?;
                }
                storage::retain_conflict(conn,record)?;result.conflicts+=1;
            }Ok(result)
        }
    }
}
fn invalid_error(message:&str)->VaultError {invalid(message)}

fn conversion_key(inner:&AppStateInner,store:&str,group:&str)->VaultResult<zeroize::Zeroizing<[u8;32]>> {
    inner.with_workspace(|_,_,_,spaces|{
        let id=store.strip_prefix("space:").ok_or_else(||invalid("Invalid conversion space"))?;
        let conn=spaces.get(id).ok_or_else(||invalid("Conversion space disappeared"))?;
        let domain=groups::inventory(conn,&storage::inventory(conn)?)?.into_iter().find(|d|d.id==group && d.protected && eligible_domain(d)).ok_or_else(||invalid("Protected domain changed; pair again"))?;
        let section=domain.records.iter().find(|r|r.entity=="sections" && !r.deleted).and_then(|r|r.payload.as_ref()?.get("id")?.as_str()).ok_or_else(||invalid("Missing conversion key reference"))?;
        inner.session.dsk_copy(section)
    })
}
fn write_staged_blob(stage:&std::path::Path,bytes:&[u8])->VaultResult<String> {
    let hash=crate::blob_store::hash_hex(bytes);let path=stage.join("blobs").join(&hash);
    if !path.exists(){let mut file=std::fs::OpenOptions::new().write(true).create_new(true).open(&path)?;file.write_all(bytes)?;file.sync_all()?;}
    Ok(hash)
}
fn domain_packet(store:&str,domain:Domain)->VaultResult<IncomingPacket> {
    let digest=crate::blob_store::hash_hex(&json(&domain)?);
    Ok(IncomingPacket{meta:PacketMeta{id:format!("{store}:{digest}:variant"),store:store.into(),group:Some(GroupMeta{id:domain.id,context:domain.context,protected:true,key_identity:domain.key_identity,eligible:true,alternative:true})},records:domain.records})
}
fn convert_plaintext_conflicts(inner:&AppStateInner,store:&str,records:Vec<Record>,record_groups:&BTreeMap<(String,String),String>,blobs:&BTreeMap<(String,String),String>,stage:&std::path::Path)->VaultResult<Vec<IncomingPacket>> {
    use zeroize::Zeroize;
    let mut ordinary=Vec::new();let mut by_group:BTreeMap<String,Vec<Record>>=BTreeMap::new();
    for record in records {
        if let Some(group)=record_groups.get(&(record.entity.clone(),record.key.clone())).cloned() {by_group.entry(group).or_default().push(record);}else{ordinary.push(record);}
    }
    inner.with_workspace(|_,_,_,spaces|{
        let id=store.strip_prefix("space:").ok_or_else(||invalid("Invalid conversion space"))?;
        let conn=spaces.get_mut(id).ok_or_else(||invalid("Conversion space disappeared"))?;
        let current=groups::inventory(conn,&storage::inventory(conn)?)?;let mut result=Vec::new();
        for (group,mut incoming) in by_group {
            let original=current.iter().find(|d|d.id==group && d.protected && eligible_domain(d)).cloned().ok_or_else(||invalid("Protected domain changed before conflict encryption"))?;
            let section=original.records.iter().find(|r|r.entity=="sections" && !r.deleted).and_then(|r|r.payload.as_ref()?.get("id")?.as_str()).ok_or_else(||invalid("Missing conversion wrapping reference"))?.to_string();
            let key=inner.session.dsk_copy(&section)?;
            let mut candidate=original.clone();
            let mut context=original.context.clone();for record in &incoming {context=storage::union(&context,&record.context);}
            let (origin,sequence):(String,u64)=conn.query_row("UPDATE sync_state SET sequence=sequence+1 RETURNING origin,sequence",[],|r|Ok((r.get(0)?,r.get(1)?)))?;
            context.insert(origin,sequence);
            for record in &mut incoming {
                if record.entity=="sections" {continue;}
                if let Some(payload)=record.payload.as_mut().and_then(|p|p.as_object_mut()) {
                    if record.entity=="pages" {
                        for field in ["content"] {
                            let value=payload.get_mut(field).ok_or_else(||invalid("Missing plaintext page field"))?;
                            let text=value.as_str().ok_or_else(||invalid("Invalid plaintext page field"))?;
                            let encrypted=crate::notes::base64_encode(&crate::crypto::cipher::seal(text.as_bytes(),key.as_ref())?);
                            if let serde_json::Value::String(plaintext)=value {plaintext.zeroize();}*value=serde_json::json!(encrypted);
                        }
                        payload.insert("section_id".into(),serde_json::json!(&section));payload.insert("title_is_encrypted".into(),serde_json::json!(0));
                    } else if record.entity=="page_versions" {
                        let value=payload.get_mut("content").ok_or_else(||invalid("Missing plaintext history"))?;
                        let encrypted=crate::notes::base64_encode(&crate::crypto::cipher::seal(value.as_str().ok_or_else(||invalid("Invalid plaintext history"))?.as_bytes(),key.as_ref())?);
                        if let serde_json::Value::String(plaintext)=value {plaintext.zeroize();}*value=serde_json::json!(encrypted);
                    } else if record.entity=="attachments" {
                        let hash=payload["hash"].as_str().ok_or_else(||invalid("Missing conflict attachment hash"))?;
                        let encrypted=blobs.get(&(hash.into(),group.clone())).ok_or_else(||invalid("Missing encrypted conflict attachment"))?;
                        payload.insert("hash".into(),serde_json::json!(encrypted));
                    }
                }
                candidate.records.retain(|r|r.entity!=record.entity || r.key!=record.key);candidate.records.push(record.clone());
            }
            candidate.context=context.clone();for record in &mut candidate.records {record.context=context.clone();}
            candidate.records.sort_by_key(|r|(r.entity.clone(),r.key.clone()));
            validate_domain(&candidate)?;
            for hash in blob_refs(&original.records)? {
                let target=stage.join("blobs").join(&hash);
                if !target.exists(){let source=crate::blob_store::blob_path(&inner.files_dir(id),&hash);if hash_file(&source)?!=hash{return Err(invalid("Current protected attachment integrity check failed"));}std::fs::copy(source,&target)?;files::sync_file(&target)?;}
            }
            result.push(domain_packet(store,original)?);result.push(domain_packet(store,candidate)?);
        }
        if !ordinary.is_empty(){result.push(IncomingPacket{meta:PacketMeta{id:format!("{store}:ordinary"),store:store.into(),group:None},records:ordinary});}
        Ok(result)
    })
}

pub fn cleanup_snapshots(inner:&AppStateInner)->VaultResult<()> {
    let root=inner.root.parent().ok_or_else(||invalid("Missing workspace parent"))?.join("tenjee-exchange-snapshots");
    if root.exists(){std::fs::remove_dir_all(root)?;}Ok(())
}
fn pending_plaintext_pages(inner:&AppStateInner)->VaultResult<BTreeSet<(String,String)>> {
    let mut result=BTreeSet::new();
    for entry in std::fs::read_dir(stage_root(inner)?)? {let entry=entry?;
        if !entry.file_type()?.is_dir() || !entry.path().join("ready.json").is_file(){continue;}
        let manifest:Manifest=serde_json::from_slice(&std::fs::read(entry.path().join("ready.json"))?).map_err(|_|invalid("Invalid pending exchange manifest"))?;
        for packet in manifest.packets.iter().filter(|p|p.meta.group.is_none() && p.meta.store.starts_with("space:")) {for record in &packet.records {if !record.deleted{if let Some(page)=head(record)?.page{result.insert((packet.meta.store.clone(),page));}}}}
    }Ok(result)
}
pub fn discard_pending(inner:&AppStateInner)->VaultResult<()> {
    for entry in std::fs::read_dir(stage_root(inner)?)? {let entry=entry?;if entry.file_type()?.is_dir(){std::fs::remove_dir_all(entry.path())?;}}
    let cache=blob_cache(inner)?;std::fs::remove_dir_all(cache)?;cleanup_snapshots(inner)
}

fn conversion_record_groups(inner:&AppStateInner,store:&str,records:&[Record],conversions:&Conversions)->VaultResult<BTreeMap<(String,String),String>> {
    let mut result=BTreeMap::new();
    if !store.starts_with("space:") || !conversions.keys().any(|(id,_)|id==store){return Ok(result);}
    inner.with_workspace(|_,_,_,spaces|{
        let conn=spaces.get(&store[6..]).ok_or_else(||invalid("Conversion space disappeared"))?;
        for record in records {
            let page=head(record)?.page.or_else(||storage::get(conn,&record.entity,&record.key).ok().flatten().and_then(|r|head(&r).ok()?.page));
            let group=page.and_then(|page|conversions.get(&(store.into(),page)).cloned());
            let group=if group.is_none() && record.entity=="sections" {
                let id:Option<String>=conn.query_row("SELECT group_id FROM sync_members WHERE entity=?1 AND entity_key=?2",rusqlite::params![record.entity,record.key],|r|r.get(0)).optional()?;
                id.filter(|id|conversions.iter().any(|((name,_),group)|name==store && group==id))
            }else{group};
            if let Some(group)=group{result.insert((record.entity.clone(),record.key.clone()),group);}
        }Ok(result)
    })
}

#[cfg(test)]
mod visible_title_tests {
    use super::*;
    use crate::{db::migrate::{run_migrations, DbKind}, notes::{self, page_tree, pages, session::SessionManager}};

    #[test]
    fn protected_packets_allow_visible_titles_and_require_encrypted_content() {
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::connection::configure(&conn).unwrap();
        run_migrations(&mut conn, DbKind::Space.migrations()).unwrap();
        storage::install(&mut conn).unwrap();
        groups::install(&mut conn).unwrap();
        let session = SessionManager::new();
        let files = tempfile::tempdir().unwrap();
        let page = page_tree::create(&conn, &session, None, "Visible private project").unwrap();
        pages::save_page(&mut conn, &session, &page.id, &page.title, "confidential body").unwrap();
        let section = page_tree::protect(&mut conn, files.path(), &session, &page.id, "password", true).unwrap();
        let legacy_title = notes::protect_content(&session, &section, true, &page.title).unwrap();
        session.lock(&section);
        let mut domain = groups::inventory(&conn, &storage::inventory(&conn).unwrap()).unwrap().into_iter().find(|d| d.protected).unwrap();
        assert!(eligible_domain(&domain));
        validate_domain(&domain).unwrap();
        let page = domain.records.iter_mut().find(|r| r.entity == "pages" && !r.deleted).unwrap();
        let payload = page.payload.as_mut().unwrap();
        assert_eq!(payload["title"], "Visible private project");
        assert_eq!(payload["title_is_encrypted"], 0);
        assert_ne!(payload["content"], "confidential body");
        payload["title"] = serde_json::json!(legacy_title);
        payload["title_is_encrypted"] = serde_json::json!(1);
        validate_domain(&domain).unwrap();
        domain.records.iter_mut().find(|r| r.entity == "pages" && !r.deleted).unwrap().payload.as_mut().unwrap()["content"] = serde_json::json!("confidential body");
        assert!(validate_domain(&domain).is_err());
    }
}
