use super::{
    destination,
    engine::{self, Control, Intent, Progress},
    journal::Journal,
    manifest::*,
    selection,
};
use crate::{
    error::VaultResult,
    sync::auth::{self, Protocol},
};
use std::{
    net::{TcpListener, TcpStream},
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

#[derive(Default)]
struct Observer {
    status: Mutex<Progress>,
    abort_at: u64,
    decline: AtomicBool,
    fault: String,
    reject_offer: bool,
    approve_after: Option<std::time::Instant>,
    edit_source: Mutex<Option<std::path::PathBuf>>,
}
impl Control for Observer {
    fn check(&self) -> VaultResult<()> {
        if self.decline.load(Ordering::SeqCst)
            || (self.reject_offer && !self.status.lock().unwrap().preview.batch.is_empty())
            || (self.abort_at > 0 && self.status.lock().unwrap().transferred_bytes >= self.abort_at)
        {
            Err(invalid("Injected stop"))
        } else {
            Ok(())
        }
    }
    fn boundary(&self, name: &str) -> VaultResult<()> {
        if std::env::var("TENJEE_CRASH_STAGE").as_deref() == Ok(name) {
            std::process::exit(91);
        }
        if self.fault == name {
            if name == "write" {
                Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "Injected write permission failure",
                )
                .into())
            } else {
                Err(invalid(format!("Injected {name}")))
            }
        } else {
            Ok(())
        }
    }
    fn space(&self, path: &Path) -> VaultResult<u64> {
        if self.fault == "disk-full"
            && self.status.lock().unwrap().transferred_bytes >= CHUNK as u64
        {
            Ok(0)
        } else {
            Ok(fs2::available_space(path)?)
        }
    }
    fn approved(&self) -> bool {
        self.approve_after
            .is_none_or(|deadline| std::time::Instant::now() >= deadline)
    }
    fn preview(&self, preview: Preview, _: &str, _: &str, _: &str) {
        self.status.lock().unwrap().preview = preview;
    }
    fn phase(&self, _: &str) {}
    fn progress(&self, progress: Progress) {
        if self
            .approve_after
            .is_some_and(|deadline| std::time::Instant::now() < deadline)
        {
            assert_eq!(
                progress.transferred_bytes, 0,
                "Payload arrived before approval"
            );
        }
        if progress.transferred_bytes >= CHUNK as u64 {
            if let Some(source) = self.edit_source.lock().unwrap().take() {
                use std::io::Write;
                let mut file = std::fs::OpenOptions::new()
                    .write(true)
                    .open(source)
                    .unwrap();
                file.write_all(b"changed").unwrap();
                file.sync_all().unwrap();
            }
        }
        *self.status.lock().unwrap() = progress;
    }
}
fn round(
    sender: &Path,
    recipient: &Path,
    destination: &Path,
    batch: &str,
    abort: u64,
) -> (bool, bool, Progress) {
    round_with(sender, recipient, destination, batch, abort, "", false)
}
fn round_with(
    sender: &Path,
    recipient: &Path,
    destination: &Path,
    batch: &str,
    abort: u64,
    fault: &str,
    reject_offer: bool,
) -> (bool, bool, Progress) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let root = recipient.to_path_buf();
    let dest = destination.to_str().unwrap().to_string();
    let identity = destination::identity(
        &destination::absolute_dir(destination)
            .unwrap()
            .dir_metadata()
            .unwrap(),
    );
    let observer = Arc::new(Observer {
        abort_at: abort,
        fault: fault.into(),
        reject_offer,
        approve_after: if fault == "delayed-approval" {
            Some(std::time::Instant::now() + Duration::from_millis(700))
        } else {
            None
        },
        edit_source: Mutex::new(if fault == "source-edit" {
            Some(std::path::PathBuf::from(
                Journal::open(sender).unwrap().local(batch, 0).unwrap().0,
            ))
        } else {
            None
        }),
        ..Default::default()
    });
    let remote = observer.clone();
    let thread = std::thread::spawn(move || {
        let (socket, _) = listener.accept().unwrap();
        socket.set_nodelay(true).unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let mut tls = auth::server_for(
            Protocol::Files,
            socket,
            &auth::ServerIdentity::new().unwrap(),
            &id,
            "01234567",
        )
        .unwrap();
        tls.sock
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        tls.sock
            .set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        engine::exchange(
            &root,
            &Intent::Receive {
                destination: dest,
                identity,
            },
            "recipient",
            &id,
            true,
            &mut tls,
            remote.as_ref(),
        )
        .is_ok()
    });
    let socket = TcpStream::connect(address).unwrap();
    socket.set_nodelay(true).unwrap();
    let mut tls = auth::client_for(Protocol::Files, socket, "01234567").unwrap();
    tls.sock
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    tls.sock
        .set_write_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let local = Observer::default();
    let sent = engine::exchange(
        sender,
        &Intent::Send {
            batch: batch.into(),
        },
        "sender",
        &uuid::Uuid::new_v4().to_string(),
        false,
        &mut tls,
        &local,
    )
    .is_ok();
    drop(tls);
    let received = thread.join().unwrap();
    let p = observer.status.lock().unwrap().clone();
    (sent, received, p)
}
#[test]
fn authenticated_mixed_batch_and_repeat_resume_preserve_content_without_duplicates() {
    let t = tempfile::tempdir().unwrap();
    let sources = t.path().join("sources");
    let send = t.path().join("sender");
    let receive = t.path().join("recipient");
    let target = t.path().join("target");
    std::fs::create_dir_all(sources.join("folder/empty")).unwrap();
    std::fs::create_dir(&target).unwrap();
    let body = vec![0x96; CHUNK * 3 + 137];
    std::fs::write(sources.join("folder/文件.bin"), &body).unwrap();
    std::fs::write(sources.join("zero"), []).unwrap();
    std::fs::write(target.join("existing"), b"keep").unwrap();
    let j = Journal::open(&send).unwrap();
    let p = selection::prepare(
        &j,
        &[sources.join("folder"), sources.join("zero")],
        &AtomicBool::new(false),
    )
    .unwrap();
    drop(j);
    let (a, b, result) = round(&send, &receive, &target, &p.batch, 0);
    assert!(a && b);
    assert!(result.confirmed);
    assert_eq!(result.completed, p.files + p.directories);
    let j = Journal::open(&receive).unwrap();
    let output = j.field(&p.batch, "output").unwrap();
    assert_eq!(
        std::fs::read(Path::new(&output).join("folder/文件.bin")).unwrap(),
        body
    );
    assert!(Path::new(&output).join("folder/empty").is_dir());
    assert_eq!(
        std::fs::metadata(Path::new(&output).join("zero"))
            .unwrap()
            .len(),
        0
    );
    drop(j);
    let (a, b, _) = round(&send, &receive, &target, &p.batch, 0);
    assert!(a && b);
    assert_eq!(std::fs::read_dir(&target).unwrap().count(), 2);
    assert_eq!(std::fs::read(target.join("existing")).unwrap(), b"keep");
}
#[test]
fn interruption_resumes_only_verified_durable_prefix_and_rejects_edited_completed_output() {
    let t = tempfile::tempdir().unwrap();
    let source = t.path().join("large.bin");
    let send = t.path().join("sender");
    let receive = t.path().join("recipient");
    let target = t.path().join("target");
    std::fs::create_dir(&target).unwrap();
    let body = vec![0x5d; CHUNK * 12 + 9];
    std::fs::write(&source, &body).unwrap();
    let j = Journal::open(&send).unwrap();
    let p = selection::prepare(&j, &[source], &AtomicBool::new(false)).unwrap();
    drop(j);
    let (a, b, _) = round(&send, &receive, &target, &p.batch, 10 * CHUNK as u64);
    assert!(!a && !b);
    let j = Journal::open(&receive).unwrap();
    assert_eq!(j.local(&p.batch, 0).unwrap().3, 8 * CHUNK as u64);
    drop(j);
    let (a, b, _) = round(&send, &receive, &target, &p.batch, 0);
    assert!(a && b);
    let j = Journal::open(&receive).unwrap();
    let output = j.field(&p.batch, "output").unwrap();
    let file = Path::new(&output).join("large.bin");
    assert_eq!(std::fs::read(&file).unwrap(), body);
    std::fs::write(&file, b"user edited").unwrap();
    drop(j);
    let (a, b, _) = round(&send, &receive, &target, &p.batch, 0);
    assert!(!a && !b);
    assert_eq!(std::fs::read(&file).unwrap(), b"user edited");
}
#[test]
fn corrupted_partial_rolls_back_before_resume() {
    use std::io::{Seek, Write};
    let t = tempfile::tempdir().unwrap();
    let source = t.path().join("data");
    let send = t.path().join("sender");
    let receive = t.path().join("recipient");
    let target = t.path().join("target");
    std::fs::create_dir(&target).unwrap();
    let body = vec![17; CHUNK * 12];
    std::fs::write(&source, &body).unwrap();
    let j = Journal::open(&send).unwrap();
    let p = selection::prepare(&j, &[source], &AtomicBool::new(false)).unwrap();
    drop(j);
    round(&send, &receive, &target, &p.batch, 10 * CHUNK as u64);
    let j = Journal::open(&receive).unwrap();
    let output = j.field(&p.batch, "output").unwrap();
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .open(Path::new(&output).join(".tenjee-partials/0.part"))
        .unwrap();
    file.seek(std::io::SeekFrom::Start(CHUNK as u64 + 5))
        .unwrap();
    file.write_all(b"corrupt").unwrap();
    drop(file);
    drop(j);
    let (a, b, _) = round(&send, &receive, &target, &p.batch, 0);
    assert!(a && b);
    assert_eq!(
        std::fs::read(Path::new(&output).join("data")).unwrap(),
        body
    );
}

#[test]
fn publication_and_receipt_faults_resume_without_false_success_or_duplicates() {
    for fault in [
        "disk-full",
        "write",
        "file-flush",
        "journal-commit",
        "verification",
        "ready",
        "rename",
        "directory-flush",
        "publish",
        "receipt-commit",
        "receipt",
        "batch-ack",
    ] {
        let t = tempfile::tempdir().unwrap();
        let source = t.path().join("payload");
        let send = t.path().join("sender");
        let receive = t.path().join("recipient");
        let target = t.path().join("target");
        std::fs::create_dir(&target).unwrap();
        let body = vec![47; CHUNK * 2 + 91];
        std::fs::write(&source, &body).unwrap();
        let j = Journal::open(&send).unwrap();
        let p = selection::prepare(&j, &[source], &AtomicBool::new(false)).unwrap();
        drop(j);
        let (a, b, result) = round_with(&send, &receive, &target, &p.batch, 0, fault, false);
        assert!(!a && !b, "{fault}");
        assert!(!result.confirmed, "{fault}");
        let j = Journal::open(&receive).unwrap();
        let offset = j.local(&p.batch, 0).unwrap().3;
        assert!(offset <= body.len() as u64);
        if fault == "file-flush" {
            assert_eq!(offset, 0);
        }
        let output = j.field(&p.batch, "output").unwrap();
        drop(j);
        let (a, b, _) = round(&send, &receive, &target, &p.batch, 0);
        assert!(a && b, "{fault}");
        assert_eq!(
            std::fs::read(Path::new(&output).join("payload")).unwrap(),
            body,
            "{fault}"
        );
        assert_eq!(std::fs::read_dir(&target).unwrap().count(), 1);
    }
}
#[test]
fn rejected_offer_and_edited_source_never_publish_payload() {
    let t = tempfile::tempdir().unwrap();
    let source = t.path().join("payload");
    let send = t.path().join("sender");
    let receive = t.path().join("recipient");
    let target = t.path().join("target");
    std::fs::create_dir(&target).unwrap();
    std::fs::write(&source, b"original").unwrap();
    let j = Journal::open(&send).unwrap();
    let p = selection::prepare(&j, &[source.clone()], &AtomicBool::new(false)).unwrap();
    drop(j);
    let (a, b, _) = round_with(&send, &receive, &target, &p.batch, 0, "", true);
    assert!(!a && !b);
    let j = Journal::open(&receive).unwrap();
    let output = j.field(&p.batch, "output").unwrap();
    assert_eq!(j.local(&p.batch, 0).unwrap().3, 0);
    drop(j);
    assert!(!Path::new(&output).join("payload").exists());
    std::fs::write(source, b"edited source").unwrap();
    let (a, b, _) = round(&send, &receive, &target, &p.batch, 0);
    assert!(!a && !b);
    assert!(!Path::new(&output).join("payload").exists());
}

#[test]
fn missing_completed_directory_is_an_explicit_error_instead_of_false_success() {
    let t = tempfile::tempdir().unwrap();
    let source = t.path().join("empty");
    let send = t.path().join("sender");
    let receive = t.path().join("recipient");
    let target = t.path().join("target");
    std::fs::create_dir(&source).unwrap();
    std::fs::create_dir(&target).unwrap();
    let j = Journal::open(&send).unwrap();
    let p = selection::prepare(&j, &[source], &AtomicBool::new(false)).unwrap();
    drop(j);
    let (a, b, _) = round(&send, &receive, &target, &p.batch, 0);
    assert!(a && b);
    let j = Journal::open(&receive).unwrap();
    let output = j.field(&p.batch, "output").unwrap();
    drop(j);
    std::fs::remove_dir(Path::new(&output).join("empty")).unwrap();
    let (a, b, _) = round(&send, &receive, &target, &p.batch, 0);
    assert!(!a && !b);
    assert!(!Path::new(&output).join("empty").exists());
}

#[test]
#[ignore = "Worker for the process-crash recovery test"]
fn crash_child() {
    let path = |name: &str| std::path::PathBuf::from(std::env::var(name).unwrap());
    let send = path("TENJEE_CRASH_SEND");
    let receive = path("TENJEE_CRASH_RECEIVE");
    let target = path("TENJEE_CRASH_TARGET");
    let batch = std::env::var("TENJEE_CRASH_BATCH").unwrap();
    let fault = std::env::var("TENJEE_CRASH_STAGE").unwrap();
    round_with(&send, &receive, &target, &batch, 0, &fault, false);
    panic!("The requested crash boundary was not reached");
}
#[test]
fn abrupt_process_exit_at_durability_boundaries_recovers_owned_output() {
    for stage in [
        "write",
        "file-flush",
        "journal-commit",
        "verification",
        "ready",
        "rename",
        "directory-flush",
        "publish",
        "receipt-commit",
        "receipt",
        "batch-ack",
    ] {
        let t = tempfile::tempdir().unwrap();
        let send = t.path().join("sender");
        let receive = t.path().join("recipient");
        let target = t.path().join("target");
        let source = t.path().join("payload");
        std::fs::create_dir(&target).unwrap();
        let body = vec![93; CHUNK * 2 + 137];
        std::fs::write(&source, &body).unwrap();
        let j = Journal::open(&send).unwrap();
        let preview = selection::prepare(&j, &[source.clone()], &AtomicBool::new(false)).unwrap();
        drop(j);
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "file_exchange::tests::crash_child",
                "--ignored",
                "--nocapture",
            ])
            .env("TENJEE_CRASH_STAGE", stage)
            .env("TENJEE_CRASH_SEND", &send)
            .env("TENJEE_CRASH_RECEIVE", &receive)
            .env("TENJEE_CRASH_TARGET", &target)
            .env("TENJEE_CRASH_BATCH", &preview.batch)
            .output()
            .unwrap();
        assert_eq!(
            child.status.code(),
            Some(91),
            "{stage}: {}",
            String::from_utf8_lossy(&child.stderr)
        );
        let j = Journal::open(&receive).unwrap();
        let output = j.field(&preview.batch, "output").unwrap();
        drop(j);
        let (a, b, progress) = round(&send, &receive, &target, &preview.batch, 0);
        assert!(a && b && progress.confirmed, "{stage}");
        assert_eq!(
            std::fs::read(Path::new(&output).join("payload")).unwrap(),
            body,
            "{stage}"
        );
        assert_eq!(std::fs::read(source).unwrap(), body);
        assert_eq!(std::fs::read_dir(&target).unwrap().count(), 1, "{stage}");
    }
}

#[test]
fn delayed_mutual_approval_blocks_content_and_source_edits_stop_publication() {
    let t = tempfile::tempdir().unwrap();
    let send = t.path().join("sender");
    let receive = t.path().join("recipient");
    let target = t.path().join("target");
    std::fs::create_dir(&target).unwrap();
    let source = t.path().join("payload");
    std::fs::write(&source, vec![41; CHUNK * 12]).unwrap();
    let j = Journal::open(&send).unwrap();
    let p = selection::prepare(&j, &[source.clone()], &AtomicBool::new(false)).unwrap();
    drop(j);
    let began = std::time::Instant::now();
    let (a, b, _) = round_with(
        &send,
        &receive,
        &target,
        &p.batch,
        0,
        "delayed-approval",
        false,
    );
    assert!(a && b);
    assert!(began.elapsed() >= Duration::from_millis(700));
    let j = Journal::open(&send).unwrap();
    let p = selection::prepare(&j, &[source], &AtomicBool::new(false)).unwrap();
    drop(j);
    let (a, b, _) = round_with(&send, &receive, &target, &p.batch, 0, "source-edit", false);
    assert!(!a && !b);
    let j = Journal::open(&receive).unwrap();
    let output = j.field(&p.batch, "output").unwrap();
    assert!(!Path::new(&output).join("payload").exists());
}
#[test]
fn pairing_listener_can_be_the_file_sender() {
    let t = tempfile::tempdir().unwrap();
    let send = t.path().join("sender");
    let receive = t.path().join("recipient");
    let target = t.path().join("target");
    let source = t.path().join("payload");
    std::fs::create_dir(&target).unwrap();
    std::fs::write(&source, b"host sends").unwrap();
    let j = Journal::open(&send).unwrap();
    let preview = selection::prepare(&j, &[source], &AtomicBool::new(false)).unwrap();
    drop(j);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let batch = preview.batch.clone();
    let worker = std::thread::spawn(move || {
        let (socket, _) = listener.accept().unwrap();
        let session = uuid::Uuid::new_v4().to_string();
        let mut stream = auth::server_for(
            Protocol::Files,
            socket,
            &auth::ServerIdentity::new().unwrap(),
            &session,
            "01234567",
        )
        .unwrap();
        engine::exchange(
            &send,
            &Intent::Send { batch },
            "sending listener",
            &session,
            true,
            &mut stream,
            &Observer::default(),
        )
    });
    let mut stream = auth::client_for(
        Protocol::Files,
        TcpStream::connect(address).unwrap(),
        "01234567",
    )
    .unwrap();
    let observer = Observer::default();
    engine::exchange(
        &receive,
        &Intent::Receive {
            destination: target.to_string_lossy().into_owned(),
            identity: destination::identity(
                &destination::absolute_dir(&target)
                    .unwrap()
                    .dir_metadata()
                    .unwrap(),
            ),
        },
        "receiving connector",
        &uuid::Uuid::new_v4().to_string(),
        false,
        &mut stream,
        &observer,
    )
    .unwrap();
    worker.join().unwrap().unwrap();
    assert!(observer.status.lock().unwrap().confirmed);
    let j = Journal::open(&receive).unwrap();
    assert_eq!(
        std::fs::read(Path::new(&j.field(&preview.batch, "output").unwrap()).join("payload"))
            .unwrap(),
        b"host sends"
    );
}
