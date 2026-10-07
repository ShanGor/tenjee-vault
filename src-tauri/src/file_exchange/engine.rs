use super::{
    codec::{self, Type},
    destination::{self, Destination, Identity, Signature},
    journal::Journal,
    manifest::*,
};
use crate::error::VaultResult;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "lowercase")]
pub enum Intent {
    Send {
        batch: String,
    },
    Receive {
        destination: String,
        identity: Identity,
    },
}
impl Intent {
    pub fn role(&self) -> &'static str {
        match self {
            Self::Send { .. } => "send",
            Self::Receive { .. } => "receive",
        }
    }
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub preview: Preview,
    pub current: String,
    #[serde(with = "wide")]
    pub transferred_bytes: u64,
    #[serde(with = "wide")]
    pub durable_bytes: u64,
    pub completed: u32,
    pub bytes_per_second: f64,
    pub eta_seconds: Option<f64>,
    pub confirmed: bool,
}
pub trait Control {
    fn check(&self) -> VaultResult<()>;
    fn approved(&self) -> bool;
    fn preview(&self, preview: Preview, peer_label: &str, peer_platform: &str, peer_session: &str);
    fn phase(&self, phase: &str);
    fn progress(&self, progress: Progress);
    fn activity(&self) {}
    fn space(&self, path: &Path) -> VaultResult<u64> {
        Ok(fs2::available_space(path)?)
    }
    #[cfg(test)]
    fn boundary(&self, _: &str) -> VaultResult<()> {
        Ok(())
    }
}
#[derive(Serialize, Deserialize)]
struct Hello {
    protocol: String,
    session: String,
    role: String,
    label: String,
    platform: String,
    chunk: u32,
    entries: u32,
}
#[derive(Serialize, Deserialize)]
struct Offer {
    preview: Preview,
    capability: String,
}
#[derive(Serialize, Deserialize)]
struct Plan {
    digest: String,
    name: String,
    entries: u32,
}
#[derive(Serialize, Deserialize)]
struct Approval {
    binding: String,
    approved: bool,
}
#[derive(Clone, Default, Serialize, Deserialize)]
struct Receipt {
    id: u32,
    #[serde(with = "wide")]
    offset: u64,
    hash: String,
    complete: bool,
}
#[derive(Serialize, Deserialize)]
struct Begin {
    id: u32,
    #[serde(with = "wide")]
    offset: u64,
}
#[derive(Serialize, Deserialize)]
struct End {
    id: u32,
    hash: String,
}
#[derive(Serialize, Deserialize)]
struct Checkpoint {
    receipts: Vec<Receipt>,
    window: u32,
}

fn approval(stream: &mut (impl Read + Write), binding: &str, c: &impl Control) -> VaultResult<()> {
    // Symmetric send/read steps avoid indefinite waits for a remote click.
    // These transport polls do not refresh meaningful session activity.
    loop {
        c.check()?;
        let approved = c.approved();
        codec::control(
            stream,
            Type::Approval,
            &Approval {
                binding: binding.into(),
                approved,
            },
        )?;
        let remote: Approval = codec::expect(stream, Type::Approval)?;
        if remote.binding != binding {
            return Err(invalid("Peer approved a different file plan"));
        }
        if approved && remote.approved {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}
fn binding(
    local: &Hello,
    remote: &Hello,
    server: bool,
    offer: &Offer,
    plan: &Plan,
) -> VaultResult<String> {
    let (listener, connector) = if server {
        (local, remote)
    } else {
        (remote, local)
    };
    Ok(hex(&Sha256::digest(bytes(&(
        listener,
        connector,
        &offer.preview.batch,
        &offer.preview.digest,
        &offer.capability,
        plan,
    ))?)))
}
pub fn exchange(
    root: &Path,
    intent: &Intent,
    label: &str,
    session: &str,
    server: bool,
    stream: &mut (impl Read + Write),
    c: &impl Control,
) -> VaultResult<()> {
    let local = Hello {
        protocol: crate::sync::auth::Protocol::Files.id().into(),
        session: session.into(),
        role: intent.role().into(),
        label: label.into(),
        platform: std::env::consts::OS.into(),
        chunk: CHUNK as u32,
        entries: MAX_ENTRIES,
    };
    codec::control(stream, Type::Hello, &local)?;
    let remote: Hello = codec::expect(stream, Type::Hello)?;
    if remote.chunk != local.chunk
        || remote.entries != local.entries
        || remote.protocol != local.protocol
        || remote.role == local.role
        || !["send", "receive"].contains(&remote.role.as_str())
        || remote.label.len() > 128
        || remote.platform.len() > 32
        || uuid::Uuid::parse_str(&remote.session).is_err()
    {
        return Err(invalid("Incompatible file mode, roles or protocol; choose Send on one device and Receive on the other"));
    }
    let mut j = Journal::open(root)?;
    match intent {
        Intent::Send { batch } => send_batch(&mut j, batch, &local, &remote, server, stream, c),
        Intent::Receive {
            destination,
            identity,
        } => receive_batch(
            &mut j,
            destination,
            identity,
            &local,
            &remote,
            server,
            stream,
            c,
        ),
    }
}

fn publish_preview(c: &impl Control, p: &Preview, peer: &Hello) {
    c.preview(p.clone(), &peer.label, &peer.platform, &peer.session);
    c.phase("approval");
}
fn measure(p: &mut Progress, start: Instant, sent: u64) {
    let elapsed = start.elapsed().as_secs_f64();
    p.bytes_per_second = if elapsed >= 1.0 {
        sent as f64 / elapsed
    } else {
        0.0
    };
    p.eta_seconds = if elapsed >= 3.0 && p.bytes_per_second > 0.0 {
        Some(p.preview.total_bytes.saturating_sub(p.transferred_bytes) as f64 / p.bytes_per_second)
    } else {
        None
    };
}
fn read_prefix(file: &mut cap_std::fs::File, offset: u64, c: &impl Control) -> VaultResult<Sha256> {
    file.seek(SeekFrom::Start(0))?;
    let mut hash = Sha256::new();
    let mut left = offset;
    let mut buffer = vec![0; CHUNK];
    while left > 0 {
        c.check()?;
        let length = left.min(CHUNK as u64) as usize;
        file.read_exact(&mut buffer[..length])?;
        hash.update(&buffer[..length]);
        left -= length as u64;
        c.activity();
    }
    Ok(hash)
}
fn hash_file(file: &mut cap_std::fs::File, size: u64, c: &impl Control) -> VaultResult<String> {
    if file.metadata()?.len() != size {
        return Err(invalid("Retained output length changed"));
    }
    Ok(hex(&read_prefix(file, size, c)?.finalize()))
}

#[allow(clippy::too_many_arguments)]
fn send_batch(
    j: &mut Journal,
    batch: &str,
    local: &Hello,
    remote: &Hello,
    server: bool,
    s: &mut (impl Read + Write),
    c: &impl Control,
) -> VaultResult<()> {
    if j.field(batch, "role")? != "send" {
        return Err(invalid("Batch is not a sender selection"));
    }
    let offer = Offer {
        preview: j.summary(batch)?,
        capability: j.field(batch, "capability")?,
    };
    if offer.preview.entries == 0 {
        return Err(invalid("Add files or folders before entering exchange"));
    }
    codec::control(s, Type::Offer, &offer)?;
    let mut first = 0;
    while first < offer.preview.entries {
        c.check()?;
        let mut page = j.page(batch, first, 100)?;
        while bytes(&page)?.len() > codec::CONTROL {
            page.pop();
        }
        if page.is_empty() {
            return Err(invalid("Manifest entry exceeds the page limit"));
        }
        first += page.len() as u32;
        codec::control(s, Type::Page, &page)?;
    }
    let plan: Plan = codec::expect(s, Type::Plan)?;
    if plan.entries != offer.preview.entries
        || plan.digest != offer.preview.digest
        || plan.name.len() > 255
    {
        return Err(invalid("Recipient plan differs from the offer"));
    }
    // Store authenticated receiver prefix claims locally; only source rehash
    // validates them before skipping bytes. No filename/length-only resume.
    let mut next = 0;
    while next < offer.preview.entries {
        let page: Vec<Receipt> = codec::expect(s, Type::Resume)?;
        if page.is_empty() || page.len() > 100 {
            return Err(invalid("Invalid resume page"));
        }
        for receipt in &page {
            let entry = j.entry(batch, next)?;
            if receipt.id != next || receipt.offset > entry.size || receipt.hash.len() != 64 {
                return Err(invalid("Invalid resume range"));
            }
            next += 1;
        }
        record_receipts(j, batch, &page, true)?;
    }
    let mut preview = offer.preview.clone();
    preview.destination = plan.name.clone();
    publish_preview(c, &preview, remote);
    approval(s, &binding(local, remote, server, &offer, &plan)?, c)?;
    j.set(batch, "phase", "transferring")?;
    c.phase("exchanging");
    let mut progress = Progress {
        preview: preview.clone(),
        ..Progress::default()
    };
    let started = Instant::now();
    let mut sent = 0;
    let mut window_bytes = 0u64;
    let mut window_files = 0;
    let mut window = 8 * 1024 * 1024u32;
    for id in 0..preview.entries {
        c.check()?;
        let entry = j.entry(batch, id)?;
        if entry.kind == Kind::Excluded {
            continue;
        }
        progress.current = entry.path.join("/");
        c.progress(progress.clone());
        if entry.kind == Kind::Directory {
            if j.local(batch, id)?.2 == "remote-complete" {
                j.state(batch, id, "complete", "")?;
                progress.completed += 1;
                continue;
            }
            codec::send(s, Type::Begin, &bytes(&Begin { id, offset: 0 })?)?;
            window_files += 1;
        } else {
            let (source, signature, state, offset, prefix) = j.local(batch, id)?;
            let mut file = super::native::open_source(&source)?;
            let expected: Signature = decode(signature.as_bytes())?;
            if destination::signature(&file)? != expected {
                return Err(invalid(format!(
                    "Source changed: {}. Prepare a new batch",
                    entry.path.join("/")
                )));
            }
            if offset > 0 {
                c.phase("verifying");
            }
            let mut hash = read_prefix(&mut file, offset, c)?;
            if hex(&hash.clone().finalize()) != prefix {
                return Err(invalid(format!(
                    "Source prefix changed: {}. Prepare a new batch",
                    entry.path.join("/")
                )));
            }
            progress.transferred_bytes += offset;
            progress.durable_bytes += offset;
            c.phase("exchanging");
            if state == "remote-complete" {
                if offset != entry.size {
                    return Err(invalid("Invalid completed-file receipt"));
                }
                j.state(batch, id, "complete", &prefix)?;
                progress.completed += 1;
                c.progress(progress.clone());
                continue;
            }
            codec::send(s, Type::Begin, &bytes(&Begin { id, offset })?)?;
            let mut buffer = vec![0; CHUNK];
            let mut current = offset;
            while current < entry.size {
                c.check()?;
                let n = (entry.size - current).min(CHUNK as u64) as usize;
                file.read_exact(&mut buffer[..n])?;
                let digest = Sha256::digest(&buffer[..n]);
                codec::data(s, id, current, &digest, &buffer[..n])?;
                hash.update(&buffer[..n]);
                current += n as u64;
                sent += n as u64;
                window_bytes += n as u64;
                progress.transferred_bytes += n as u64;
                measure(&mut progress, started, sent);
                c.progress(progress.clone());
                if window_bytes >= window as u64 {
                    sender_window(j, batch, s, c, &mut progress, &mut window)?;
                    window_bytes = 0;
                    window_files = 0;
                }
            }
            if destination::signature(&file)? != expected {
                return Err(invalid(format!(
                    "Source changed during transfer: {}",
                    entry.path.join("/")
                )));
            }
            codec::send(
                s,
                Type::End,
                &bytes(&End {
                    id,
                    hash: hex(&hash.finalize()),
                })?,
            )?;
            window_files += 1;
        }
        if window_files >= 1024 {
            sender_window(j, batch, s, c, &mut progress, &mut window)?;
            window_bytes = 0;
            window_files = 0;
        }
    }
    sender_window(j, batch, s, c, &mut progress, &mut window)?;
    codec::control(s, Type::Finish, &preview.digest)?;
    let result: Preview = codec::expect(s, Type::Result)?;
    if result.batch != batch
        || result.digest != preview.digest
        || result.completed != preview.files + preview.directories
        || result.phase != "complete"
    {
        return Err(invalid("Recipient did not confirm every accepted entry"));
    }
    j.set(batch, "phase", "complete")?;
    progress.preview = result;
    progress.confirmed = true;
    progress.completed = progress.preview.completed;
    c.progress(progress);
    c.phase("finished");
    Ok(())
}
fn sender_window(
    j: &mut Journal,
    batch: &str,
    s: &mut (impl Read + Write),
    c: &impl Control,
    p: &mut Progress,
    window: &mut u32,
) -> VaultResult<()> {
    c.check()?;
    let started = Instant::now();
    codec::control(s, Type::Window, window)?;
    let ack: Checkpoint = codec::expect(s, Type::Checkpoint)?;
    if ack.receipts.len() > 1025 || !(8 * 1024 * 1024..=32 * 1024 * 1024).contains(&ack.window) {
        return Err(invalid("Invalid checkpoint window"));
    }
    record_receipts(j, batch, &ack.receipts, false)?;
    // Grow windows for costly acknowledgement latency without buffering an
    // entire window. Bytes are streamed, not queued in memory.
    if started.elapsed() > Duration::from_millis(40) {
        *window = (*window * 2).min(32 * 1024 * 1024);
    }
    let (completed,durable):(u32,u64)=j.conn.query_row("SELECT COALESCE(SUM(state='complete'),0),COALESCE(SUM(CAST(offset AS INTEGER)),0) FROM entries WHERE batch=?1",[batch],|r|Ok((r.get(0)?,r.get(1)?)))?;
    p.completed = completed;
    p.durable_bytes = durable;
    c.progress(p.clone());
    Ok(())
}

fn record_receipts(
    j: &Journal,
    batch: &str,
    receipts: &[Receipt],
    resume: bool,
) -> VaultResult<()> {
    let tx = j.conn.unchecked_transaction()?;
    let mut seen = std::collections::BTreeSet::new();
    for receipt in receipts {
        let entry = j.entry(batch, receipt.id)?;
        if !seen.insert(receipt.id)
            || receipt.offset > entry.size
            || receipt.hash.len() != 64
            || !receipt.hash.bytes().all(|b| b.is_ascii_hexdigit())
            || receipt.complete && receipt.offset != entry.size
            || entry.kind == Kind::Excluded && (receipt.complete || receipt.offset != 0)
        {
            return Err(invalid("Invalid checkpoint receipt"));
        }
        let state = if receipt.complete {
            if resume {
                "remote-complete"
            } else {
                "complete"
            }
        } else {
            "pending"
        };
        tx.execute(
            "UPDATE entries SET offset=?3,state=?4,hash=?5 WHERE batch=?1 AND id=?2",
            rusqlite::params![
                batch,
                receipt.id,
                receipt.offset.to_string(),
                state,
                receipt.hash
            ],
        )?;
    }
    tx.commit()?;
    Ok(())
}

fn recovered(
    j: &mut Journal,
    batch: &str,
    entry: &Entry,
    d: &Destination,
    c: &impl Control,
) -> VaultResult<Receipt> {
    let (_, _, state, offset, hash) = j.local(batch, entry.id)?;
    let empty = hex(&Sha256::digest([]));
    if entry.kind == Kind::Directory && state == "complete" {
        d.verify_directory(entry)?;
    }
    if entry.kind != Kind::File {
        return Ok(Receipt {
            id: entry.id,
            offset: 0,
            hash: empty,
            complete: state == "complete",
        });
    }
    if state == "complete" || state == "ready" {
        match d.existing(entry) {
            Ok(mut f) => {
                if hash_file(&mut f, entry.size, c)? != hash {
                    return Err(invalid(
                        "Completed destination file changed; do not overwrite it",
                    ));
                }
                j.state(batch, entry.id, "complete", &hash)?;
                return Ok(Receipt {
                    id: entry.id,
                    offset: entry.size,
                    hash,
                    complete: true,
                });
            }
            Err(crate::error::VaultError::Io(e))
                if e.kind() == std::io::ErrorKind::NotFound && state == "ready" => {}
            Err(e) => return Err(e),
        }
    }
    let mut file = d.partial(entry.id)?;
    let length = file.metadata()?.len();
    let mut good = 0u64;
    let mut full = Sha256::new();
    let mut buffer = vec![0; CHUNK];
    j.visit_chunks(batch, entry.id, |start, size, hash| {
        c.check()?;
        if start != good
            || size == 0
            || size as usize > CHUNK
            || good + size as u64 > offset
            || good + size as u64 > length
        {
            return Ok(false);
        }
        file.read_exact(&mut buffer[..size as usize])?;
        c.activity();
        if hex(&Sha256::digest(&buffer[..size as usize])) != hash {
            return Ok(false);
        }
        full.update(&buffer[..size as usize]);
        good += size as u64;
        Ok(true)
    })?;
    if good != offset {
        j.rollback(batch, entry.id, good)?;
    }
    file.set_len(good)?;
    file.sync_data()?;
    Ok(Receipt {
        id: entry.id,
        offset: good,
        hash: hex(&full.finalize()),
        complete: false,
    })
}
struct Incoming {
    entry: Entry,
    file: cap_std::fs::File,
    hash: Sha256,
    offset: u64,
    durable: u64,
    pending: Vec<(u64, u32, String)>,
    last: Instant,
}
fn checkpoint(
    j: &mut Journal,
    batch: &str,
    current: &mut Incoming,
    c: &impl Control,
) -> VaultResult<Receipt> {
    #[cfg(not(test))]
    let _ = c;
    current.file.sync_data()?;
    #[cfg(test)]
    c.boundary("file-flush")?;
    j.checkpoint(batch, current.entry.id, current.offset, &current.pending)?;
    #[cfg(test)]
    c.boundary("journal-commit")?;
    current.pending.clear();
    current.durable = current.offset;
    current.last = Instant::now();
    Ok(Receipt {
        id: current.entry.id,
        offset: current.offset,
        hash: hex(&current.hash.clone().finalize()),
        complete: false,
    })
}

#[allow(clippy::too_many_arguments)]
fn receive_batch(
    j: &mut Journal,
    selected: &str,
    identity: &Identity,
    local: &Hello,
    remote: &Hello,
    server: bool,
    s: &mut (impl Read + Write),
    c: &impl Control,
) -> VaultResult<()> {
    let offer: Offer = codec::expect(s, Type::Offer)?;
    let batch = &offer.preview.batch;
    if offer.preview.entries == 0
        || offer.preview.entries > MAX_ENTRIES
        || offer.preview.digest.len() != 64
    {
        return Err(invalid("Invalid offered batch"));
    }
    if super::native::destination_identity(selected)? != *identity {
        return Err(invalid("Selected destination changed; select it again"));
    }
    let existing = j.exists(batch)?;
    if existing
        && (j.field(batch, "role")? != "receive"
            || j.field(batch, "capability")? != offer.capability
            || j.field(batch, "digest")? != offer.preview.digest
            || j.field(batch, "destination")? != selected)
    {
        return Err(invalid(
            "Resume identity or destination differs; use the original batch and destination",
        ));
    }
    if !existing {
        j.create("receive", Some(batch), Some(&offer.capability), selected)?;
    }
    let metadata_result = (|| -> VaultResult<Preview> {
        let mut transaction = if !existing {
            Some(j.conn.unchecked_transaction()?)
        } else {
            None
        };
        let mut received = 0u32;
        let mut metadata = 0usize;
        while received < offer.preview.entries {
            c.check()?;
            let page: Vec<Entry> = codec::expect(s, Type::Page)?;
            if page.is_empty() || page.len() > 100 {
                return Err(invalid("Invalid manifest page"));
            }
            for entry in page {
                if entry.id != received
                    || received >= offer.preview.entries
                    || entry.reason.len() > 4096
                    || entry.kind != Kind::File && entry.size != 0
                {
                    return Err(invalid("Invalid manifest entry"));
                }
                path(&entry.path)?;
                metadata += bytes(&entry)?.len();
                if metadata as u64 > META_LIMIT {
                    return Err(invalid("Manifest exceeds 64 MiB"));
                }
                if existing {
                    if j.entry(batch, received)? != entry {
                        return Err(invalid("Resume manifest differs"));
                    }
                } else {
                    j.add(batch, &entry, "", "")?;
                }
                received += 1;
            }
        }
        let summary = j.summary(batch)?;
        j.validate_tree(batch)?;
        if j.digest(batch)? != offer.preview.digest
            || summary.files != offer.preview.files
            || summary.directories != offer.preview.directories
            || summary.excluded != offer.preview.excluded
            || summary.total_bytes != offer.preview.total_bytes
        {
            return Err(invalid("Offer totals or digest differ from its manifest"));
        }
        j.set(batch, "digest", &offer.preview.digest)?;
        if let Some(transaction) = transaction.take() {
            transaction.commit()?;
        }
        drop(transaction);
        Ok(summary)
    })();
    let summary = match metadata_result {
        Ok(summary) => summary,
        Err(error) => {
            if !existing {
                let _ = j.delete(batch);
            }
            return Err(error);
        }
    };
    let received = summary.entries;
    if super::native::available_space(selected)?
        < summary
            .total_bytes
            .checked_add(RESERVE)
            .ok_or_else(|| invalid("Disk estimate overflow"))?
        && !existing
    {
        return Err(invalid("Insufficient destination space for this batch"));
    }
    let d = if existing && !j.field(batch, "output")?.is_empty() {
        Destination::reopen(
            Path::new(&j.field(batch, "output")?),
            &decode::<Identity>(j.field(batch, "identity")?.as_bytes())?,
        )?
    } else {
        let name = format!(
            "Tenjee Received {}-{}",
            chrono::Local::now().format("%Y%m%d"),
            &batch[..8]
        );
        let d = if super::native::provider(selected) {
            Destination::provider_create(selected, &name, batch)?
        } else {
            Destination::create(Path::new(selected), &name)?
        };
        j.set(batch, "output", &d.location())?;
        j.set(
            batch,
            "identity",
            &String::from_utf8(bytes(&destination::identity(&d.output.dir_metadata()?))?).unwrap(),
        )?;
        d
    };
    if d.provider.is_some() {
        let mut largest = 0;
        for id in 0..summary.entries {
            largest = largest.max(j.entry(batch, id)?.size);
        }
        if c.space(&d.absolute)?
            < largest
                .checked_add(RESERVE)
                .ok_or_else(|| invalid("Staging estimate overflow"))?
        {
            return Err(invalid(
                "Insufficient app-private space for the largest Android file",
            ));
        }
    }
    let plan = Plan {
        digest: offer.preview.digest.clone(),
        name: d.name()?,
        entries: received,
    };
    codec::control(s, Type::Plan, &plan)?;
    let mut preview = j.summary(batch)?;
    let mut initial_bytes = 0u64;
    for start in (0..received).step_by(100) {
        let mut page = Vec::new();
        for entry in j.page(batch, start, 100)? {
            let receipt = recovered(j, batch, &entry, &d, c)?;
            initial_bytes += receipt.offset;
            page.push(receipt);
        }
        codec::control(s, Type::Resume, &page)?;
    }
    preview.completed = j.summary(batch)?.completed;
    publish_preview(c, &preview, remote);
    approval(s, &binding(local, remote, server, &offer, &plan)?, c)?;
    j.set(batch, "phase", "transferring")?;
    c.phase("exchanging");
    let mut progress = Progress {
        preview: preview.clone(),
        transferred_bytes: initial_bytes,
        durable_bytes: initial_bytes,
        completed: preview.completed,
        ..Progress::default()
    };
    let mut active: Option<Incoming> = None;
    let mut receipts = Vec::new();
    let started = Instant::now();
    let mut incoming = 0u64;
    let mut credit = 0u64;
    let mut window_work = false;
    let mut final_empty_window = false;
    let mut last_entry: Option<u32> = None;
    loop {
        c.check()?;
        if receipts.len() > 1024 {
            return Err(invalid("Too many unacknowledged entries"));
        }
        let frame = codec::receive(s)?;
        match frame.kind {
            Type::Begin => {
                window_work = true;
                if active.is_some() {
                    return Err(invalid("Entry started before previous file ended"));
                }
                let start: Begin = decode(&frame.data)?;
                if last_entry.is_some_and(|id| start.id <= id) || start.id >= received {
                    return Err(invalid("Duplicated or out-of-order entry"));
                }
                last_entry = Some(start.id);
                let entry = j.entry(batch, start.id)?;
                let (_, _, state, offset, _) = j.local(batch, start.id)?;
                if state == "complete" || entry.kind == Kind::Excluded || start.offset != offset {
                    return Err(invalid("Entry does not match its approved resume state"));
                }
                progress.current = entry.path.join("/");
                c.progress(progress.clone());
                if entry.kind == Kind::Directory {
                    d.directory(&entry)?;
                    j.state(batch, entry.id, "complete", "")?;
                    receipts.push(Receipt {
                        id: entry.id,
                        offset: 0,
                        hash: hex(&Sha256::digest([])),
                        complete: true,
                    });
                    progress.completed += 1;
                } else {
                    let mut file = d.partial(entry.id)?;
                    let hash = read_prefix(&mut file, offset, c)?;
                    active = Some(Incoming {
                        entry,
                        file,
                        hash,
                        offset,
                        durable: offset,
                        pending: Vec::new(),
                        last: Instant::now(),
                    });
                }
                c.progress(progress.clone());
            }
            Type::Data => {
                window_work = true;
                let current = active
                    .as_mut()
                    .ok_or_else(|| invalid("Data outside a file"))?;
                let id = u32::from_be_bytes(frame.data[..4].try_into().unwrap());
                let offset = u64::from_be_bytes(frame.data[4..12].try_into().unwrap());
                let data = &frame.data[44..];
                credit += data.len() as u64;
                if credit > 32 * 1024 * 1024 {
                    return Err(invalid("Peer exceeded the data window"));
                }
                if data.len()
                    != (current.entry.size.saturating_sub(current.offset)).min(CHUNK as u64)
                        as usize
                    || id != current.entry.id
                    || offset != current.offset
                    || offset
                        .checked_add(data.len() as u64)
                        .is_none_or(|end| end > current.entry.size)
                {
                    return Err(invalid("Invalid file data range"));
                }
                let hash = Sha256::digest(data);
                if hash.as_slice() != &frame.data[12..44] {
                    return Err(invalid("Chunk integrity verification failed"));
                }
                if c.space(&d.absolute)? < data.len() as u64 + RESERVE {
                    return Err(invalid(
                        "Insufficient storage; retain partial progress and free space",
                    ));
                }
                #[cfg(test)]
                c.boundary("write")?;
                current.file.write_all(data)?;
                current.hash.update(data);
                current
                    .pending
                    .push((offset, data.len() as u32, hex(&hash)));
                current.offset += data.len() as u64;
                incoming += data.len() as u64;
                progress.transferred_bytes += data.len() as u64;
                if current.offset - current.durable >= 8 * 1024 * 1024
                    || current.last.elapsed() > Duration::from_secs(2)
                {
                    let old = current.durable;
                    checkpoint(j, batch, current, c)?;
                    progress.durable_bytes += current.durable - old;
                }
                measure(&mut progress, started, incoming);
                c.progress(progress.clone());
            }
            Type::End => {
                window_work = true;
                let end: End = decode(&frame.data)?;
                let mut current = active.take().ok_or_else(|| invalid("End outside a file"))?;
                c.phase("verifying");
                if end.id != current.entry.id
                    || current.offset != current.entry.size
                    || hex(&current.hash.clone().finalize()) != end.hash
                {
                    return Err(invalid(
                        "File length or final integrity verification failed",
                    ));
                }
                #[cfg(test)]
                c.boundary("verification")?;
                let old = current.durable;
                let mut receipt = checkpoint(j, batch, &mut current, c)?;
                progress.durable_bytes += current.durable - old;
                current.file.sync_all()?;
                j.state(batch, current.entry.id, "ready", &end.hash)?;
                c.phase("saving");
                #[cfg(test)]
                c.boundary("ready")?;
                d.publish_with(&current.entry, |name| {
                    #[cfg(test)]
                    {
                        c.boundary(name)
                    }
                    #[cfg(not(test))]
                    {
                        let _ = name;
                        Ok(())
                    }
                })?;
                #[cfg(test)]
                c.boundary("publish")?;
                j.state(batch, current.entry.id, "complete", &end.hash)?;
                #[cfg(test)]
                c.boundary("receipt-commit")?;
                receipt.complete = true;
                receipt.hash = end.hash;
                receipts.push(receipt);
                progress.completed += 1;
                c.progress(progress.clone());
                c.phase("exchanging");
            }
            Type::Window => {
                if !window_work {
                    let done = j.summary(batch)?;
                    if active.is_some()
                        || final_empty_window
                        || done.completed != done.files + done.directories
                    {
                        return Err(invalid("Window outside meaningful transfer progress"));
                    }
                    final_empty_window = true;
                }
                window_work = false;
                credit = 0;
                let window: u32 = decode(&frame.data)?;
                if !(8 * 1024 * 1024..=32 * 1024 * 1024).contains(&window) {
                    return Err(invalid("Invalid window size"));
                }
                if let Some(current) = active.as_mut() {
                    let old = current.durable;
                    receipts.push(checkpoint(j, batch, current, c)?);
                    progress.durable_bytes += current.durable - old;
                }
                if receipts.len() > 1025 {
                    return Err(invalid("Too many unacknowledged file completions"));
                }
                #[cfg(test)]
                c.boundary("receipt")?;
                codec::control(
                    s,
                    Type::Checkpoint,
                    &Checkpoint {
                        receipts: std::mem::take(&mut receipts),
                        window,
                    },
                )?;
                c.progress(progress.clone());
            }
            Type::Finish => {
                let digest: String = decode(&frame.data)?;
                if active.is_some() || !receipts.is_empty() || digest != preview.digest {
                    return Err(invalid("Batch finished outside its approved state"));
                }
                let mut result = j.summary(batch)?;
                if result.completed != result.files + result.directories {
                    return Err(invalid("Batch has incomplete accepted entries"));
                }
                j.set(batch, "phase", "complete")?;
                result.phase = "complete".into();
                #[cfg(test)]
                c.boundary("batch-ack")?;
                let mut wire = result.clone();
                wire.destination = plan.name.clone();
                codec::control(s, Type::Result, &wire)?;
                progress.preview = result;
                progress.confirmed = true;
                c.progress(progress);
                c.phase("finished");
                return Ok(());
            }
            _ => return Err(invalid("Unexpected file transfer frame")),
        }
    }
}

#[cfg(test)]
mod protocol_tests {
    use super::*;
    struct Script {
        input: std::io::Cursor<Vec<u8>>,
        output: Vec<u8>,
    }
    impl Read for Script {
        fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
            self.input.read(b)
        }
    }
    impl Write for Script {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.output.write(b)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    struct Auto;
    impl Control for Auto {
        fn check(&self) -> VaultResult<()> {
            Ok(())
        }
        fn approved(&self) -> bool {
            true
        }
        fn preview(&self, _: Preview, _: &str, _: &str, _: &str) {}
        fn phase(&self, _: &str) {}
        fn progress(&self, _: Progress) {}
    }
    #[test]
    fn conflicting_roles_and_protocols_fail_before_offering_manifest() {
        for (role, protocol) in [
            ("send", crate::sync::auth::Protocol::Files.id()),
            ("receive", "unsupported-protocol"),
        ] {
            let hello = Hello {
                protocol: protocol.into(),
                session: uuid::Uuid::new_v4().to_string(),
                role: role.into(),
                label: "untrusted discovery label".into(),
                platform: "test".into(),
                chunk: CHUNK as u32,
                entries: MAX_ENTRIES,
            };
            let mut input = Vec::new();
            codec::control(&mut input, Type::Hello, &hello).unwrap();
            let mut script = Script {
                input: std::io::Cursor::new(input),
                output: Vec::new(),
            };
            let root = tempfile::tempdir().unwrap();
            assert!(exchange(
                root.path(),
                &Intent::Send {
                    batch: uuid::Uuid::new_v4().to_string()
                },
                "local",
                &uuid::Uuid::new_v4().to_string(),
                true,
                &mut script,
                &Auto
            )
            .is_err());
            let mut sent = script.output.as_slice();
            assert_eq!(codec::receive(&mut sent).unwrap().kind, Type::Hello);
            assert!(sent.is_empty());
            assert!(!root.path().join("transfers.sqlite").exists());
        }
    }
    #[test]
    fn approval_binding_covers_destination_naming_and_session() {
        let hello = |role: &str| Hello {
            protocol: crate::sync::auth::Protocol::Files.id().into(),
            session: uuid::Uuid::new_v4().to_string(),
            role: role.into(),
            label: "peer".into(),
            platform: "test".into(),
            chunk: CHUNK as u32,
            entries: MAX_ENTRIES,
        };
        let a = hello("send");
        let mut b = hello("receive");
        let offer = Offer {
            preview: Preview {
                batch: uuid::Uuid::new_v4().to_string(),
                digest: "1".repeat(64),
                ..Default::default()
            },
            capability: "2".repeat(64),
        };
        let mut plan = Plan {
            digest: offer.preview.digest.clone(),
            name: "approved root".into(),
            entries: 1,
        };
        let original = binding(&a, &b, true, &offer, &plan).unwrap();
        assert_eq!(original, binding(&b, &a, false, &offer, &plan).unwrap());
        plan.name = "other root".into();
        assert_ne!(original, binding(&a, &b, true, &offer, &plan).unwrap());
        plan.name = "approved root".into();
        b.session = uuid::Uuid::new_v4().to_string();
        assert_ne!(original, binding(&a, &b, true, &offer, &plan).unwrap());
    }
    #[test]
    fn tiny_chunks_invalid_ranges_and_empty_window_floods_are_rejected_before_writes() {
        for fault in ["tiny", "id", "offset", "empty-windows"] {
            let t = tempfile::tempdir().unwrap();
            let target = t.path().join("target");
            std::fs::create_dir(&target).unwrap();
            let receiver = t.path().join("recipient");
            let j = Journal::open(&t.path().join("sender")).unwrap();
            let batch = j.create("send", None, None, "").unwrap();
            let entry = Entry {
                id: 0,
                path: vec!["payload".into()],
                kind: Kind::File,
                size: CHUNK as u64 + 1,
                reason: String::new(),
            };
            j.add(&batch, &entry, "", "").unwrap();
            j.set(&batch, "digest", &j.digest(&batch).unwrap()).unwrap();
            let offer = Offer {
                preview: j.summary(&batch).unwrap(),
                capability: j.field(&batch, "capability").unwrap(),
            };
            let local = Hello {
                protocol: crate::sync::auth::Protocol::Files.id().into(),
                session: uuid::Uuid::new_v4().to_string(),
                role: "receive".into(),
                label: "local".into(),
                platform: std::env::consts::OS.into(),
                chunk: CHUNK as u32,
                entries: MAX_ENTRIES,
            };
            let remote = Hello {
                protocol: local.protocol.clone(),
                session: uuid::Uuid::new_v4().to_string(),
                role: "send".into(),
                label: "remote".into(),
                platform: "test".into(),
                chunk: CHUNK as u32,
                entries: MAX_ENTRIES,
            };
            let plan = Plan {
                digest: offer.preview.digest.clone(),
                name: format!(
                    "Tenjee Received {}-{}",
                    chrono::Local::now().format("%Y%m%d"),
                    &batch[..8]
                ),
                entries: 1,
            };
            let mut input = Vec::new();
            codec::control(&mut input, Type::Hello, &remote).unwrap();
            codec::control(&mut input, Type::Offer, &offer).unwrap();
            codec::control(&mut input, Type::Page, &vec![entry]).unwrap();
            codec::control(
                &mut input,
                Type::Approval,
                &Approval {
                    binding: binding(&local, &remote, true, &offer, &plan).unwrap(),
                    approved: true,
                },
            )
            .unwrap();
            codec::control(&mut input, Type::Begin, &Begin { id: 0, offset: 0 }).unwrap();
            if fault == "empty-windows" {
                codec::control(&mut input, Type::Window, &(8 * 1024 * 1024u32)).unwrap();
                codec::control(&mut input, Type::Window, &(8 * 1024 * 1024u32)).unwrap();
            } else {
                let data = vec![1; if fault == "tiny" { 1 } else { CHUNK }];
                codec::data(
                    &mut input,
                    if fault == "id" { 1 } else { 0 },
                    if fault == "offset" { u64::MAX } else { 0 },
                    &Sha256::digest(&data),
                    &data,
                )
                .unwrap();
            }
            let mut script = Script {
                input: std::io::Cursor::new(input),
                output: Vec::new(),
            };
            let identity = destination::identity(
                &destination::absolute_dir(&target)
                    .unwrap()
                    .dir_metadata()
                    .unwrap(),
            );
            let error = exchange(
                &receiver,
                &Intent::Receive {
                    destination: target.to_string_lossy().into_owned(),
                    identity,
                },
                "local",
                &local.session,
                true,
                &mut script,
                &Auto,
            )
            .unwrap_err();
            assert!(
                error.to_string().contains(if fault == "empty-windows" {
                    "meaningful transfer"
                } else {
                    "data range"
                }),
                "{fault}: {error}"
            );
            let j = Journal::open(&receiver).unwrap();
            assert_eq!(j.local(&batch, 0).unwrap().3, 0);
            assert!(!Path::new(&j.field(&batch, "output").unwrap())
                .join("payload")
                .exists());
        }
    }
}
