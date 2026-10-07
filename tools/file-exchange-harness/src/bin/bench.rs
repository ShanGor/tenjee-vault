//! Owned-data loopback benchmark. Route/device acceptance is recorded separately.
use rand::{RngCore, SeedableRng};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tenjee_file_exchange_harness::{
    error::VaultResult,
    file_exchange::{
        codec::{self, Type},
        destination::{self, Destination},
        engine::{self, Control, Intent, Progress},
        journal::Journal,
        manifest::*,
        selection,
    },
    sync::auth::{self, Protocol},
};
const CODE: &str = "01234567";
#[derive(Default)]
struct Timings {
    current: String,
    start: Option<Instant>,
    phases: BTreeMap<String, f64>,
    bytes: u64,
    confirmed: bool,
}
#[derive(Default)]
struct Observer(Mutex<Timings>);
impl Observer {
    fn report(&self) -> serde_json::Value {
        self.phase("ended");
        let t = self.0.lock().unwrap();
        json!({"phasesSeconds":t.phases,"bytes":t.bytes.to_string(),"confirmed":t.confirmed})
    }
}
impl Control for Observer {
    fn check(&self) -> VaultResult<()> {
        Ok(())
    }
    fn approved(&self) -> bool {
        true
    }
    fn preview(&self, _: Preview, _: &str, _: &str, _: &str) {}
    fn phase(&self, phase: &str) {
        let mut t = self.0.lock().unwrap();
        if t.current == phase {
            return;
        }
        if let Some(start) = t.start {
            let old = t.current.clone();
            *t.phases.entry(old).or_default() += start.elapsed().as_secs_f64();
        }
        t.current = phase.into();
        t.start = Some(Instant::now());
    }
    fn progress(&self, p: Progress) {
        let mut t = self.0.lock().unwrap();
        t.bytes = p.transferred_bytes;
        t.confirmed = p.confirmed;
    }
}
fn rss() -> u64 {
    #[cfg(target_os = "linux")]
    {
        let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
        for line in status.lines() {
            if let Some(v) = line.strip_prefix("VmRSS:") {
                return v
                    .split_whitespace()
                    .next()
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(0)
                    * 1024;
            }
        }
    }
    0
}
fn cpu() -> f64 {
    #[cfg(target_os = "linux")]
    {
        let mut t = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        if unsafe { libc::clock_gettime(libc::CLOCK_PROCESS_CPUTIME_ID, &mut t) } == 0 {
            return t.tv_sec as f64 + t.tv_nsec as f64 / 1e9;
        }
    }
    0.0
}
fn baseline_receive(
    root: &Path,
    target: &Path,
    entry: Entry,
    s: &mut (impl Read + Write),
    observer: &Observer,
) -> VaultResult<()> {
    let mut j = Journal::open(root)?;
    let batch = j.create("receive", None, None, "")?;
    j.add(&batch, &entry, "", "")?;
    let d = Destination::create(target, "baseline")?;
    let mut file = d.partial(0)?;
    let mut offset = 0u64;
    let mut full = Sha256::new();
    let mut pending = Vec::new();
    let mut durable = 0;
    let mut last = Instant::now();
    observer.phase("exchanging");
    loop {
        let frame = codec::receive(s)?;
        match frame.kind {
            Type::Data => {
                let at = u64::from_be_bytes(frame.data[4..12].try_into().unwrap());
                let data = &frame.data[44..];
                let hash = Sha256::digest(data);
                if at != offset
                    || hash.as_slice() != &frame.data[12..44]
                    || offset + data.len() as u64 > entry.size
                {
                    return Err(invalid("Baseline range/hash mismatch"));
                }
                if fs2::available_space(target)? < RESERVE + data.len() as u64 {
                    return Err(invalid("Baseline storage exhausted"));
                }
                file.write_all(data)?;
                full.update(data);
                pending.push((offset, data.len() as u32, hex(&hash)));
                offset += data.len() as u64;
                if offset - durable >= 8 * 1024 * 1024 || last.elapsed() > Duration::from_secs(2) {
                    file.sync_data()?;
                    j.checkpoint(&batch, 0, offset, &pending)?;
                    pending.clear();
                    durable = offset;
                    last = Instant::now();
                }
            }
            Type::Window => {
                file.sync_data()?;
                j.checkpoint(&batch, 0, offset, &pending)?;
                pending.clear();
                durable = offset;
                last = Instant::now();
                codec::control(s, Type::Checkpoint, &offset.to_string())?;
            }
            Type::End => {
                observer.phase("verifying");
                let hash: String = decode(&frame.data)?;
                if offset != entry.size || hex(&full.finalize()) != hash {
                    return Err(invalid("Baseline final hash mismatch"));
                }
                file.sync_data()?;
                j.checkpoint(&batch, 0, offset, &pending)?;
                file.sync_all()?;
                j.state(&batch, 0, "ready", &hash)?;
                observer.phase("saving");
                d.publish(&entry)?;
                j.state(&batch, 0, "complete", &hash)?;
                codec::control(s, Type::Result, &hash)?;
                observer.progress(Progress {
                    transferred_bytes: offset,
                    confirmed: true,
                    ..Progress::default()
                });
                observer.phase("finished");
                return Ok(());
            }
            _ => return Err(invalid("Unexpected baseline frame")),
        }
    }
}
fn baseline_send(
    source: &Path,
    entry: &Entry,
    s: &mut (impl Read + Write),
    observer: &Observer,
) -> VaultResult<()> {
    let mut file = destination::source(source)?;
    let signature = destination::signature(&file)?;
    let mut hash = Sha256::new();
    let mut offset = 0u64;
    let mut bytes = vec![0; CHUNK];
    let mut credit = 8 * 1024 * 1024u32;
    let mut pending = 0u64;
    observer.phase("exchanging");
    while offset < entry.size {
        let n = (entry.size - offset).min(CHUNK as u64) as usize;
        file.read_exact(&mut bytes[..n])?;
        let range = Sha256::digest(&bytes[..n]);
        codec::data(s, 0, offset, &range, &bytes[..n])?;
        hash.update(&bytes[..n]);
        offset += n as u64;
        pending += n as u64;
        observer.progress(Progress {
            transferred_bytes: offset,
            ..Progress::default()
        });
        if pending >= credit as u64 {
            let start = Instant::now();
            codec::control(s, Type::Window, &credit)?;
            let ack: String = codec::expect(s, Type::Checkpoint)?;
            if ack != offset.to_string() {
                return Err(invalid("Baseline checkpoint mismatch"));
            }
            if start.elapsed() > Duration::from_millis(40) {
                credit = (credit * 2).min(32 * 1024 * 1024);
            }
            pending = 0;
        }
    }
    if destination::signature(&file)? != signature {
        return Err(invalid("Baseline source changed"));
    }
    let hash = hex(&hash.finalize());
    codec::control(s, Type::End, &hash)?;
    let saved: String = codec::expect(s, Type::Result)?;
    if saved != hash {
        return Err(invalid("Baseline receipt mismatch"));
    }
    observer.progress(Progress {
        transferred_bytes: offset,
        confirmed: true,
        ..Progress::default()
    });
    observer.phase("finished");
    Ok(())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg = |key: &str, default: &str| {
        args.windows(2)
            .find(|p| p[0] == key)
            .map(|p| p[1].clone())
            .unwrap_or_else(|| default.into())
    };
    let mode = arg("--mode", "files");
    let size: u64 = arg("--bytes", "67108864").parse()?;
    let count: u32 = arg("--files", "1").parse()?;
    if count == 0 || count > 100_000 {
        return Err("files must be 1..100000".into());
    }
    if mode != "files" && mode != "baseline" {
        return Err("mode must be files or baseline".into());
    }
    if mode == "baseline" && count != 1 {
        return Err("The raw baseline measures one large file; small-file acceptance has no relative baseline".into());
    }
    let storage = PathBuf::from(arg("--storage-dir", std::env::temp_dir().to_str().unwrap()));
    let owned = tempfile::tempdir_in(&storage)?;
    let t = owned.path();
    let source = t.join("source");
    std::fs::create_dir(&source)?;
    let target = t.join("output");
    std::fs::create_dir(&target)?;
    let mut rng = rand::rngs::StdRng::seed_from_u64(42);
    let mut buffer = vec![0; CHUNK];
    let generation = Instant::now();
    for id in 0..count {
        let parent = if count > 1 {
            source.join(format!("group-{}", id / 1000))
        } else {
            source.clone()
        };
        std::fs::create_dir_all(&parent)?;
        let name = if count > 1 && id == 0 {
            ".hidden".to_string()
        } else if count > 1 && id + 1 == count {
            "文件-é.bin".to_string()
        } else {
            format!("file-{id:05}")
        };
        let mut file = std::fs::File::create(parent.join(name))?;
        let mut left = size;
        while left > 0 {
            let n = left.min(CHUNK as u64) as usize;
            rng.fill_bytes(&mut buffer[..n]);
            file.write_all(&buffer[..n])?;
            left -= n as u64;
        }
        file.sync_all()?;
    }
    if count > 1 {
        std::fs::create_dir(source.join("empty"))?;
    }
    drop(buffer);
    let generated = generation.elapsed().as_secs_f64();
    let sender = t.join("sender");
    let recipient = t.join("recipient");
    let j = Journal::open(&sender)?;
    let scan = Instant::now();
    let roots = if count == 1 {
        vec![source.join("file-00000")]
    } else {
        vec![source.clone()]
    };
    let preview = selection::prepare(&j, &roots, &AtomicBool::new(false))?;
    let scan_seconds = scan.elapsed().as_secs_f64();
    let entry = j.entry(&preview.batch, 0)?;
    drop(j);
    let base_rss = rss();
    let peak = Arc::new(AtomicU64::new(base_rss));
    let stop = Arc::new(AtomicBool::new(false));
    let peak_copy = peak.clone();
    let stop_copy = stop.clone();
    let sampler = std::thread::spawn(move || {
        while !stop_copy.load(Ordering::Relaxed) {
            peak_copy.fetch_max(rss(), Ordering::Relaxed);
            std::thread::sleep(Duration::from_millis(10));
        }
    });
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    let identity = destination::identity(&destination::absolute_dir(&target)?.dir_metadata()?);
    let incoming = Arc::new(Observer::default());
    let outgoing = Observer::default();
    let observer = incoming.clone();
    let mode_copy = mode.clone();
    let entry_copy = entry.clone();
    let started = Instant::now();
    let cpu_start = cpu();
    let receiver = std::thread::spawn(move || -> VaultResult<()> {
        let (socket, _) = listener.accept()?;
        socket.set_nodelay(true)?;
        let session = uuid::Uuid::new_v4().to_string();
        let mut tls = auth::server_for(
            Protocol::Files,
            socket,
            &auth::ServerIdentity::new()?,
            &session,
            CODE,
        )?;
        tls.sock.set_read_timeout(Some(Duration::from_secs(120)))?;
        tls.sock.set_write_timeout(Some(Duration::from_secs(120)))?;
        if mode_copy == "files" {
            engine::exchange(
                &recipient,
                &Intent::Receive {
                    destination: target.to_string_lossy().into_owned(),
                    identity,
                },
                "benchmark recipient",
                &session,
                true,
                &mut tls,
                observer.as_ref(),
            )
        } else {
            baseline_receive(&recipient, &target, entry_copy, &mut tls, observer.as_ref())
        }
    });
    let socket = TcpStream::connect(address)?;
    socket.set_nodelay(true)?;
    let mut tls = auth::client_for(Protocol::Files, socket, CODE)?;
    tls.sock.set_read_timeout(Some(Duration::from_secs(120)))?;
    tls.sock.set_write_timeout(Some(Duration::from_secs(120)))?;
    let result = if mode == "files" {
        engine::exchange(
            &sender,
            &Intent::Send {
                batch: preview.batch,
            },
            "benchmark sender",
            &uuid::Uuid::new_v4().to_string(),
            false,
            &mut tls,
            &outgoing,
        )
    } else {
        baseline_send(&roots[0], &entry, &mut tls, &outgoing)
    };
    drop(tls);
    result?;
    receiver.join().map_err(|_| "Recipient panicked")??;
    let elapsed = started.elapsed().as_secs_f64();
    let cpu_seconds = cpu() - cpu_start;
    stop.store(true, Ordering::Relaxed);
    sampler.join().unwrap();
    println!(
        "{}",
        json!({"mode":mode,"route":"IPv4 loopback, no shaping","storageDirectory":storage,"platform":std::env::consts::OS,"bytesPerFile":size.to_string(),"fileCount":count,"totalBytes":preview.total_bytes.to_string(),"generationSeconds":generated,"scanSeconds":scan_seconds,"sessionSeconds":elapsed,"bytesPerSecond":preview.total_bytes as f64/elapsed,"processCpuSeconds":cpu_seconds,"rssBeforeBytes":base_rss,"peakRssBytes":peak.load(Ordering::Relaxed),"incrementalRssBytes":peak.load(Ordering::Relaxed).saturating_sub(base_rss),"memoryScope":"Both peers in one process; excludes dataset generation/scan","sender":outgoing.report(),"recipient":incoming.report()})
    );
    Ok(())
}
