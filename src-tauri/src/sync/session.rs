use super::{auth, engine, network};
use crate::commands::AppState;
use crate::error::{VaultError, VaultResult};
use mdns_sd::{IfKind, ServiceDaemon, ServiceEvent, ServiceInfo};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::{atomic::{AtomicBool, Ordering}, Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::Manager as _;
use zeroize::{Zeroize, Zeroizing};

const SERVICE: &str = "_tenjee-vault._tcp.local.";

#[derive(Clone, Serialize, Deserialize)]
pub struct Peer { pub session: String, pub label: String, pub platform: String, pub addresses: Vec<String> }

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub session: String, pub phase: String, pub label: String, pub addresses: Vec<String>,
    pub code: Option<String>, pub expires_in: u64, pub attempts_left: u8,
    pub peers: Vec<Peer>, pub peer: Option<Peer>, pub local_summary: Option<engine::Summary>, pub peer_summary: Option<engine::Summary>,
    pub result: Option<engine::ExchangeResult>, pub message: String,
    pub transferred_records: usize, pub total_records: usize, pub attachment_bytes: u64,
}

struct Mutable {
    status: Status, code: Zeroizing<String>, expires: Instant,
    busy: bool, approved: bool, touched: Instant,
    peers: BTreeMap<String,Peer>,
}

pub struct Session {
    pub id: String,
    network: network::Network,
    #[allow(dead_code)]
    app: tauri::AppHandle,
    #[allow(dead_code)]
    native_token: String,
    inner: Mutex<Mutable>,
    cancelled: AtomicBool,
    accepting: AtomicBool,
    sockets: Mutex<Vec<TcpStream>>,
    listeners: Mutex<Vec<TcpListener>>,
    discovery: Mutex<Option<ServiceDaemon>>,
    identity: Mutex<Option<auth::ServerIdentity>>,
    workers: Mutex<Vec<std::thread::JoinHandle<()>>>,
}

impl Session {
    fn state(&self) -> std::sync::MutexGuard<'_,Mutable> { self.inner.lock().unwrap_or_else(|e|e.into_inner()) }
    pub fn check(&self) -> VaultResult<()> {
        if self.cancelled.load(Ordering::SeqCst) { Err(VaultError::Validation("Exchange stopped; start a fresh pairing to resume".into())) } else { Ok(()) }
    }
    pub fn progress_total(&self,total:usize){self.state().status.total_records=total;}
    pub fn progress_record(&self){let mut state=self.state();state.status.transferred_records+=1;state.touched=Instant::now();}
    pub fn progress_bytes(&self,bytes:u64){let mut state=self.state();state.status.attachment_bytes+=bytes;state.touched=Instant::now();}
    pub fn touch(&self) { self.state().touched = Instant::now(); }
    pub fn phase(&self, phase: &str, message: &str) { let mut state=self.state(); state.status.phase=phase.into(); state.status.message=message.into(); state.touched=Instant::now(); }
    pub fn result(&self, result: engine::ExchangeResult) { self.state().status.result=Some(result); }
    fn close_discovery(&self) {
        self.accepting.store(false,Ordering::SeqCst);
        self.listeners.lock().unwrap().clear();
        if let Some(daemon) = self.discovery.lock().unwrap().take() { let _=daemon.stop_browse(SERVICE); let _=daemon.shutdown(); }
    }
    pub fn cancel(&self, message: &str) {
        self.cancelled.store(true,Ordering::SeqCst);
        self.close_discovery();
        { let mut state=self.state(); state.code.zeroize(); state.status.code=None; if !matches!(state.status.phase.as_str(),"finished"|"failed") { state.status.phase="stopped".into(); state.status.message=message.into(); } }
        self.listeners.lock().unwrap().clear();
        self.identity.lock().unwrap().take();
        for socket in self.sockets.lock().unwrap().drain(..) { let _=socket.shutdown(Shutdown::Both); }
        #[cfg(target_os="android")]
        {
            let app=self.app.clone(); let token=self.native_token.clone();
            tauri::async_runtime::spawn(async move { let _=crate::mobile::call(app,"endExchange",serde_json::json!({"token":token})).await; });
        }
    }
    fn register_socket(&self, socket: &TcpStream) -> VaultResult<()> {
        self.check()?;
        let mut sockets=self.sockets.lock().unwrap();
        self.check()?;
        sockets.push(socket.try_clone()?);
        Ok(())
    }
    fn status(&self) -> Status {
        let state=self.state(); let mut status=state.status.clone();
        status.expires_in=state.expires.saturating_duration_since(Instant::now()).as_secs();
        status.code=if !state.code.is_empty() && status.phase=="discovering" { Some(state.code.to_string()) } else {None};
        status.peers=state.peers.values().cloned().collect(); status
    }
    fn join(&self) {
        let handles = std::mem::take(&mut *self.workers.lock().unwrap());
        for handle in handles { let _=handle.join(); }
    }
    fn authenticated(&self) {
        { let mut state=self.state(); state.code.zeroize(); state.busy=true; state.status.code=None; state.status.phase="negotiating".into(); }
        self.close_discovery();
        self.identity.lock().unwrap().take();
    }
    fn wait_approval(&self) -> VaultResult<()> {
        loop { self.check()?; if self.state().approved { return Ok(()); } std::thread::sleep(Duration::from_millis(100)); }
    }
    fn exchange(&self, app: &tauri::AppHandle, stream: &mut (impl std::io::Read + std::io::Write), receiver: bool) -> VaultResult<()> {
        let state=app.state::<AppState>();
        engine::recover(&state.inner)?;
        let snapshot=engine::snapshot(&state.inner)?;
        let local=engine::hello(&state.inner, &self.state().status.label, &snapshot)?;
        auth::send(stream,&local)?;
        let remote: engine::Hello=auth::receive(stream,32768)?;
        engine::validate_hello(&local,&remote)?;
        {
            let mut status=self.state();
            status.status.peer=Some(Peer{session:remote.replica.clone(),label:remote.label.clone(),platform:remote.platform.clone(),addresses:vec![]});
            status.status.local_summary=Some(local.summary.clone()); status.status.peer_summary=Some(remote.summary.clone());
        }
        self.phase("approval","");
        self.wait_approval()?;
        let binding=if receiver { engine::approval_binding(&remote,&local)? } else { engine::approval_binding(&local,&remote)? };
        auth::send(stream,&serde_json::json!({"approved":true,"scope":binding}))?;
        let approval: serde_json::Value=auth::receive(stream,4096)?;
        if approval["approved"]!=true || approval["scope"]!=binding { return Err(VaultError::Validation("Peer did not approve the same full-workspace scope".into())); }
        self.phase("exchanging","");
        let result=engine::exchange(&state.inner,stream,&snapshot,&remote.replica,receiver,self)?;
        self.result(result);
        self.phase("finished","");
        Ok(())
    }
}

#[derive(Default)]
pub struct ExchangeManager(Mutex<Option<Arc<Session>>>,AtomicBool);
impl ExchangeManager {
    fn current(&self) -> VaultResult<Arc<Session>> { self.0.lock().unwrap().clone().ok_or_else(||VaultError::Validation("Enter exchange mode first".into())) }
    pub fn cancel(&self, reason: &str) { self.1.store(false,Ordering::SeqCst); if let Some(session)=self.0.lock().unwrap().as_ref() { session.cancel(reason); } }
    pub fn stop(&self) { self.1.store(false,Ordering::SeqCst); if let Some(session)=self.0.lock().unwrap().clone() { session.cancel("Exchange stopped; pair again to resume"); session.join(); } }
}

fn start(app: tauri::AppHandle, label: String, native_token: String, interface: Option<String>, port: u16) -> VaultResult<Status> {
    if !app.state::<ExchangeManager>().1.load(Ordering::SeqCst) { return Err(VaultError::Validation("Exchange start was interrupted".into())); }
    if label.trim().is_empty() || label.len()>128 || label.chars().any(char::is_control) { return Err(VaultError::Validation("Enter a device label of at most 128 bytes".into())); }
    let mut network=network::Network::select(interface.as_deref())?;
    let listeners=network.listen(port)?;
    let addresses=listeners.iter().map(|l|l.local_addr().map(|a|a.to_string())).collect::<Result<Vec<_>,_>>()?;
    let id=uuid::Uuid::new_v4().to_string();
    let code=Zeroizing::new(format!("{:08}",rand::rngs::OsRng.gen_range(0..100_000_000u32)));
    let identity=auth::ServerIdentity::new()?;
    let session=Arc::new(Session{id:id.clone(),network,app:app.clone(),native_token,inner:Mutex::new(Mutable{status:Status{session:id.clone(),phase:"discovering".into(),label:label.clone(),addresses,attempts_left:5,..Status::default()},code,expires:Instant::now()+Duration::from_secs(300),busy:false,approved:false,touched:Instant::now(),peers:BTreeMap::new()}),cancelled:AtomicBool::new(false),accepting:AtomicBool::new(true),sockets:Mutex::new(vec![]),listeners:Mutex::new(vec![]),discovery:Mutex::new(None),identity:Mutex::new(Some(identity)),workers:Mutex::new(vec![])});
    *session.listeners.lock().unwrap()=listeners;
    {
        let manager=app.state::<ExchangeManager>(); let mut current=manager.0.lock().unwrap();
        if !manager.1.load(Ordering::SeqCst) { return Err(VaultError::Validation("Exchange start was interrupted".into())); }
        *current=Some(session.clone());
    }
    let mut active_discovery=session.discovery.lock().unwrap();
    session.check()?;
    let interfaces=session.network.discovery_interfaces();
    let discovery=if interfaces.is_empty() { None } else { ServiceDaemon::new().ok() };
    if let Some(daemon)=discovery {
        let _=daemon.disable_interface(IfKind::All);
        for interface in &interfaces { let _=daemon.enable_interface(IfKind::Name(interface.name.clone())); }
        for (index,listener) in session.listeners.lock().unwrap().iter().enumerate() {
            let address=listener.local_addr()?;
            if !interfaces.iter().any(|interface| interface.ip()==address.ip()) { continue; }
            let instance=format!("{}-{}",id,index);
            let properties=[("session",id.as_str()),("label",label.as_str()),("platform",std::env::consts::OS),("version","1")];
            if let Ok(info)=ServiceInfo::new(SERVICE,&instance,&format!("tenjee-{}.local.",id),address.ip(),address.port(),&properties[..]) { let _=daemon.register(info); }
        }
        if let Ok(events)=daemon.browse(SERVICE) {
            let owner=session.clone();
            session.workers.lock().unwrap().push(std::thread::spawn(move || {
                while owner.accepting.load(Ordering::SeqCst) && owner.check().is_ok() {
                    if let Ok(event)=events.recv_timeout(Duration::from_millis(200)) {
                        match event {
                            ServiceEvent::ServiceResolved(info) => {
                                let id=info.get_property_val_str("session").unwrap_or("");
                                if id==owner.id || uuid::Uuid::parse_str(id).is_err() || info.get_property_val_str("version")!=Some("1") { continue; }
                                let mut addresses=Vec::new();
                                for ip in info.get_addresses() {
                                    let address=match ip { mdns_sd::ScopedIp::V4(v4)=>std::net::SocketAddr::new((*v4.addr()).into(),info.get_port()),mdns_sd::ScopedIp::V6(v6)=>std::net::SocketAddr::V6(std::net::SocketAddrV6::new(*v6.addr(),info.get_port(),0,v6.scope_id().index)),_=>continue };
                                    if owner.network.allows(address) { addresses.push(address.to_string()); }
                                }
                                let mut state=owner.state();
                                if !addresses.is_empty() && state.peers.len()<100 { state.peers.insert(info.get_fullname().to_string(),Peer{session:id.into(),label:info.get_property_val_str("label").unwrap_or("Tenjee Vault").chars().take(64).collect(),platform:info.get_property_val_str("platform").unwrap_or("unknown").chars().take(32).collect(),addresses}); }
                            }
                            ServiceEvent::ServiceRemoved(_,name)=>{ owner.state().peers.remove(&name); }
                            _=>{}
                        }
                    }
                }
            }));
        }
        if session.check().is_ok() { *active_discovery=Some(daemon); } else { let _=daemon.shutdown(); }
    } else { session.state().status.message="Nearby discovery is unavailable on this network. Enter the peer hostname/IP and port; for Tailscale, use its MagicDNS name.".into(); }
    drop(active_discovery);
    let listener_count=session.listeners.lock().unwrap().len();
    for index in 0..listener_count {
        let owner=session.clone(); let app=app.clone();
        session.workers.lock().unwrap().push(std::thread::spawn(move || {
            while owner.accepting.load(Ordering::SeqCst) && owner.check().is_ok() {
                let accepted={ let listeners=owner.listeners.lock().unwrap(); match listeners.get(index) { Some(listener)=>listener.accept(),None=>break } };
                match accepted {
                    Ok((socket,address))=>{
                        if !socket.local_addr().is_ok_and(|local| owner.network.allows_incoming(local,address)) { continue; }
                        let code={ let mut state=owner.state(); if state.busy || state.status.attempts_left==0 || Instant::now()>=state.expires { continue; } state.busy=true; Zeroizing::new(state.code.to_string()) };
                        if owner.register_socket(&socket).is_err() { break; }
                        let identity=owner.identity.lock().unwrap().clone();
                        let authenticated=identity.and_then(|identity|auth::server(socket,&identity,&owner.id,&code).ok());
                        if let Some(mut stream)=authenticated {
                            if Instant::now()>=owner.state().expires || owner.check().is_err() { owner.cancel("Pairing code expired"); break; }
                            owner.authenticated();
                            let _=stream.sock.set_read_timeout(Some(Duration::from_secs(1800)));
                            let result=owner.exchange(&app,&mut stream,true);
                            if let Err(error)=result { owner.phase("failed",&serde_json::to_string(&error.payload()).unwrap_or_else(|_|"Exchange failed".into())); }
                            owner.cancel("Exchange ended; pair again to resume");
                            break;
                        } else {
                            let expired={ let mut state=owner.state(); state.busy=false; state.status.attempts_left=state.status.attempts_left.saturating_sub(1); state.status.message="Pairing failed; check the code and remaining attempts".into(); state.status.attempts_left==0 };
                            owner.sockets.lock().unwrap().clear();
                            if expired { owner.cancel("Pairing code invalidated after five failed attempts"); break; }
                        }
                    }
                    Err(error) if error.kind()==std::io::ErrorKind::WouldBlock=>std::thread::sleep(Duration::from_millis(100)),
                    Err(_)=>{ owner.cancel("Local listener interrupted"); break; }
                }
            }
        }));
    }
    let owner=session.clone();
    session.workers.lock().unwrap().push(std::thread::spawn(move || {
        while owner.check().is_ok() {
            let expired={ let state=owner.state(); (state.status.phase=="discovering" && Instant::now()>=state.expires) || state.touched.elapsed()>Duration::from_secs(1800) };
            if expired { owner.cancel("Exchange expired; generate a fresh pairing code"); break; }
            std::thread::sleep(Duration::from_millis(250));
        }
    }));
    Ok(session.status())
}

#[tauri::command]
pub async fn exchange_enter_cmd(app: tauri::AppHandle, label: String, interface: Option<String>, port: Option<u16>) -> VaultResult<Status> {
    static ENTER:std::sync::OnceLock<tauri::async_runtime::Mutex<()>>=std::sync::OnceLock::new();
    let _enter=ENTER.get_or_init(||tauri::async_runtime::Mutex::new(())).lock().await;
    if !cfg!(debug_assertions) {return Err(VaultError::Validation("Device exchange awaits protocol and physical-device validation before release".into()));}
    let owner=app.clone();
    tauri::async_runtime::spawn_blocking(move ||owner.state::<ExchangeManager>().stop()).await.map_err(|_|VaultError::Validation("Exchange cleanup interrupted".into()))?;
    let token=uuid::Uuid::new_v4().to_string();
    app.state::<ExchangeManager>().1.store(true,Ordering::SeqCst);
    #[cfg(target_os="android")]
    {
        let owner=app.clone();
        let channel=tauri::ipc::Channel::<serde_json::Value>::new(move |_| { owner.state::<ExchangeManager>().cancel("App suspended; fresh pairing is required"); Ok(()) });
        if let Err(error)=crate::mobile::call(app.clone(),"beginExchange",serde_json::json!({"suspended":channel,"token":token})).await {
            app.state::<ExchangeManager>().1.store(false,Ordering::SeqCst);
            let _=crate::mobile::call(app.clone(),"endExchange",serde_json::json!({"token":token})).await;
            return Err(error);
        }
    }
    let owner=app.clone(); let session_token=token.clone();
    let result=tauri::async_runtime::spawn_blocking(move || start(owner,label,session_token,interface,port.unwrap_or(0))).await.map_err(|_|VaultError::Validation("Cannot start exchange mode".into()))?;
    #[cfg(target_os="android")]
    if result.is_err() { let _=crate::mobile::call(app,"endExchange",serde_json::json!({"token":token})).await; }
    result
}

#[tauri::command]
pub fn exchange_status_cmd(app: tauri::AppHandle) -> Status { app.state::<ExchangeManager>().current().map(|s|s.status()).unwrap_or_else(|_|Status{phase:"idle".into(),..Status::default()}) }

#[tauri::command]
pub async fn exchange_stop_cmd(app: tauri::AppHandle) -> VaultResult<()> {
    #[cfg(target_os="android")]
    let token=app.state::<ExchangeManager>().current().map(|s|s.native_token.clone()).unwrap_or_default();
    let owner=app.clone();
    tauri::async_runtime::spawn_blocking(move ||owner.state::<ExchangeManager>().stop()).await.map_err(|_|VaultError::Validation("Exchange cleanup interrupted".into()))?;
    #[cfg(target_os="android")]
    crate::mobile::call(app,"endExchange",serde_json::json!({"token":token})).await?;
    Ok(())
}

#[tauri::command]
pub fn exchange_approve_cmd(app: tauri::AppHandle) -> VaultResult<()> {
    let session=app.state::<ExchangeManager>().current()?; session.check()?;
    let mut state=session.state();
    if state.status.phase!="approval" { return Err(VaultError::Validation("The exchange preview is not ready".into())); }
    state.approved=true; state.touched=Instant::now(); Ok(())
}

#[tauri::command]
pub fn exchange_connect_cmd(app: tauri::AppHandle, address: String, code: String) -> VaultResult<()> {
    let code=Zeroizing::new(code);
    let session=app.state::<ExchangeManager>().current()?; session.check()?;
    { let mut state=session.state(); if state.busy || state.status.phase!="discovering" { return Err(VaultError::Validation("Another peer is already pairing".into())); } state.busy=true; state.status.phase="pairing".into(); }
    let owner=session.clone();
    session.workers.lock().unwrap().push(std::thread::spawn(move || {
        let result=(|| {
            owner.check()?;
            let addresses=owner.network.resolve(&address,||owner.check())?;
            let socket=owner.network.connect(&addresses,||owner.check())?;
            owner.register_socket(&socket)?;
            let mut stream=auth::client(socket,&code)?;
            owner.check()?;
            owner.authenticated();
            stream.sock.set_read_timeout(Some(Duration::from_secs(1800)))?;
            owner.exchange(&app,&mut stream,false)
        })();
        if let Err(error)=result { owner.phase("failed",&serde_json::to_string(&error.payload()).unwrap_or_else(|_|"Exchange failed".into())); }
        owner.cancel("Exchange ended; pair again to resume");
    }));
    Ok(())
}

#[tauri::command]
pub fn exchange_available_cmd()->bool {cfg!(debug_assertions)}

#[tauri::command]
pub async fn exchange_networks_cmd()->VaultResult<Vec<network::NetworkOption>> {
    tauri::async_runtime::spawn_blocking(network::options).await.map_err(|_|VaultError::Validation("Cannot list network interfaces".into()))?
}

pub fn before_protection(app:&tauri::AppHandle,inner:&crate::commands::AppStateInner)->VaultResult<()> {
    app.state::<ExchangeManager>().stop();engine::protection_barrier(inner)
}

#[tauri::command]
pub async fn exchange_discard_pending_cmd(app:tauri::AppHandle)->VaultResult<()> {
    let owner=app.clone();
    tauri::async_runtime::spawn_blocking(move ||{owner.state::<ExchangeManager>().stop();engine::discard_pending(&owner.state::<AppState>().inner)?;
        if let Ok(session)=owner.state::<ExchangeManager>().current(){let mut state=session.state();state.status.phase="stopped".into();state.status.result=None;state.status.message="Pending exchange files cleared; committed data is kept".into();}
        Ok(())}).await.map_err(|_|VaultError::Validation("Exchange cleanup interrupted".into()))?
}
