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
pub struct Peer { pub session: String, pub label: String, pub platform: String, pub addresses: Vec<String>, #[serde(default)] pub mode:String, #[serde(default)] pub protocol:String, #[serde(default)] pub role:String }

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub session: String, pub phase: String, pub label: String, pub addresses: Vec<String>,
    pub code: Option<String>, pub expires_in: u64, pub attempts_left: u8,
    pub peers: Vec<Peer>, pub peer: Option<Peer>, pub local_summary: Option<engine::Summary>, pub peer_summary: Option<engine::Summary>,
    pub result: Option<engine::ExchangeResult>, pub message: String,
    pub transferred_records: usize, pub total_records: usize, pub attachment_bytes: u64,
    pub mode:String, pub files:Option<crate::file_exchange::engine::Progress>,
}

struct Mutable {
    status: Status, code: Zeroizing<String>, expires: Instant,
    busy: bool, approved: bool, touched: Instant,
    peers: BTreeMap<String,Peer>,
}

pub struct Session {
    pub id: String,
    network: network::Network,
    file_intent:Option<crate::file_exchange::engine::Intent>,
    file_root:std::path::PathBuf,
    #[cfg(target_os="android")]
    app: tauri::AppHandle,
    #[cfg(target_os="android")]
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
    fn claim_pairing(&self)->Option<Zeroizing<String>> {
        let mut state=self.state();
        if state.busy||state.status.attempts_left==0||state.code.is_empty()||Instant::now()>=state.expires{return None;}
        state.busy=true;Some(Zeroizing::new(state.code.to_string()))
    }
    fn pairing_failed(&self, error: &VaultError)->bool {
        let exhausted={let mut state=self.state();state.busy=false;state.status.attempts_left=state.status.attempts_left.saturating_sub(1);state.status.message=serde_json::to_string(&error.payload()).unwrap_or_else(|_|"Pairing failed".into());state.status.attempts_left==0};
        self.sockets.lock().unwrap().clear();
        if exhausted{self.cancel("Pairing code invalidated after five failed attempts");}exhausted
    }
    fn expired(&self)->bool {
        let state=self.state();
        (state.status.phase=="discovering"&&Instant::now()>=state.expires)||state.touched.elapsed()>Duration::from_secs(1800)||(self.file_intent.is_some()&&matches!(state.status.phase.as_str(),"exchanging"|"verifying"|"saving")&&state.touched.elapsed()>Duration::from_secs(120))
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
        if let Some(intent)=&self.file_intent {
            let label=self.state().status.label.clone();
            let mut observed=ObservedFileIo{stream,session:self};
            return crate::file_exchange::engine::exchange(&self.file_root,intent,&label,&self.id,receiver,&mut observed,self);
        }
        let state=app.state::<AppState>();
        self.exchange_workspace(&state.inner,stream,receiver)
    }
    fn exchange_workspace(&self, inner: &crate::commands::AppStateInner, stream: &mut (impl std::io::Read + std::io::Write), receiver: bool) -> VaultResult<()> {
        let result = self.exchange_workspace_inner(inner, stream, receiver);
        if let Err(error) = &result {
            // Best effort, without replacing the original local failure. A peer
            // failure is already reported by the other device; do not echo it.
            if self.check().is_ok() && !matches!(error, VaultError::Validation(detail) if detail.starts_with("Peer exchange failed: ")) {
                let _ = auth::send_failure(stream, error);
            }
        }
        result
    }
    fn exchange_workspace_inner(&self, inner: &crate::commands::AppStateInner, stream: &mut (impl std::io::Read + std::io::Write), receiver: bool) -> VaultResult<()> {
        engine::recover(inner)?;
        let snapshot=engine::snapshot(inner)?;
        let local=engine::hello(inner, &self.state().status.label, &snapshot)?;
        auth::send(stream,&local)?;
        let remote: engine::Hello=auth::receive(stream,32768)?;
        engine::validate_hello(&local,&remote)?;
        {
            let mut status=self.state();
            status.status.peer=Some(Peer{session:remote.replica.clone(),label:remote.label.clone(),platform:remote.platform.clone(),addresses:vec![],mode:"vault".into(),protocol:auth::Protocol::Vault.id().into(),role:String::new()});
            status.status.local_summary=Some(local.summary.clone()); status.status.peer_summary=Some(remote.summary.clone());
        }
        self.phase("approval","");
        self.wait_approval()?;
        let binding=if receiver { engine::approval_binding(&remote,&local)? } else { engine::approval_binding(&local,&remote)? };
        auth::send(stream,&serde_json::json!({"approved":true,"scope":binding}))?;
        let approval: serde_json::Value=auth::receive(stream,4096)?;
        if approval["approved"]!=true || approval["scope"]!=binding { return Err(VaultError::Validation("Peer did not approve the same full-workspace scope".into())); }
        self.phase("exchanging","");
        let result=engine::exchange(inner,stream,&snapshot,&remote.replica,receiver,self)?;
        self.result(result);
        self.phase("finished","");
        Ok(())
    }
}


impl crate::file_exchange::engine::Control for Session {
    fn check(&self)->VaultResult<()> { Session::check(self) }
    fn approved(&self)->bool {self.state().approved}
    fn preview(&self,preview:crate::file_exchange::manifest::Preview,label:&str,platform:&str,peer_session:&str){let mut state=self.state();state.status.peer=Some(Peer{session:peer_session.into(),label:label.into(),platform:platform.into(),addresses:vec![],mode:"files".into(),protocol:auth::Protocol::Files.id().into(),role:self.file_intent.as_ref().map(|i|if i.role()=="send"{"receive"}else{"send"}).unwrap_or("").into()});state.status.files=Some(crate::file_exchange::engine::Progress{preview,..Default::default()});}
    fn phase(&self,phase:&str){Session::phase(self,phase,"");}
    fn progress(&self,progress:crate::file_exchange::engine::Progress){let mut state=self.state();state.status.files=Some(progress);state.touched=Instant::now();}
    fn activity(&self){self.touch();}
}
struct ObservedFileIo<'a,T>{stream:&'a mut T,session:&'a Session}
impl<T:std::io::Read> std::io::Read for ObservedFileIo<'_,T>{
    fn read(&mut self,buffer:&mut[u8])->std::io::Result<usize>{
        self.session.check().map_err(std::io::Error::other)?;let n=self.stream.read(buffer)?;
        if n>0&&matches!(self.session.state().status.phase.as_str(),"exchanging"|"verifying"|"saving"){self.session.touch();}Ok(n)
    }
}
impl<T:std::io::Write> std::io::Write for ObservedFileIo<'_,T>{
    fn write(&mut self,buffer:&[u8])->std::io::Result<usize>{self.session.check().map_err(std::io::Error::other)?;let n=self.stream.write(buffer)?;if n>0&&self.session.state().status.phase=="exchanging"{self.session.touch();}Ok(n)}
    fn flush(&mut self)->std::io::Result<()>{self.stream.flush()}
}

#[derive(Default)]
pub struct ExchangeManager(Mutex<Option<Arc<Session>>>,AtomicBool);
impl ExchangeManager {
    fn current(&self) -> VaultResult<Arc<Session>> { self.0.lock().unwrap().clone().ok_or_else(||VaultError::Validation("Enter exchange mode first".into())) }
    pub fn cancel(&self, reason: &str) { self.1.store(false,Ordering::SeqCst); if let Some(session)=self.0.lock().unwrap().as_ref() { session.cancel(reason); } }
    pub fn stop(&self) { self.1.store(false,Ordering::SeqCst); if let Some(session)=self.0.lock().unwrap().clone() { session.cancel("Exchange stopped; pair again to resume"); session.join(); } }
}

fn start(app: tauri::AppHandle, label: String, native_token: String, interface: Option<String>, port: u16, file_intent: Option<crate::file_exchange::engine::Intent>) -> VaultResult<Status> {
    #[cfg(not(target_os="android"))]
    let _=native_token;
    if !app.state::<ExchangeManager>().1.load(Ordering::SeqCst) { return Err(VaultError::Validation("Exchange start was interrupted".into())); }
    if label.trim().is_empty() || label.len()>128 || label.chars().any(char::is_control) { return Err(VaultError::Validation("Enter a device label of at most 128 bytes".into())); }
    let mut network=network::Network::select(interface.as_deref())?;
    let listeners=network.listen(port)?;
    let addresses=listeners.iter().map(|l|l.local_addr().map(|a|a.to_string())).collect::<Result<Vec<_>,_>>()?;
    let id=uuid::Uuid::new_v4().to_string();
    let code=Zeroizing::new(format!("{:08}",rand::rngs::OsRng.gen_range(0..100_000_000u32)));
    let identity=auth::ServerIdentity::new()?;
    let file_root=if file_intent.is_some(){crate::file_exchange::journal::root(&app)?}else{std::path::PathBuf::new()};
    let mode=if file_intent.is_some(){"files"}else{"vault"};
    let session=Arc::new(Session{id:id.clone(),network,file_intent,file_root,#[cfg(target_os="android")] app:app.clone(),#[cfg(target_os="android")] native_token,inner:Mutex::new(Mutable{status:Status{session:id.clone(),phase:"discovering".into(),mode:mode.into(),label:label.clone(),addresses,attempts_left:5,..Status::default()},code,expires:Instant::now()+Duration::from_secs(300),busy:false,approved:false,touched:Instant::now(),peers:BTreeMap::new()}),cancelled:AtomicBool::new(false),accepting:AtomicBool::new(true),sockets:Mutex::new(vec![]),listeners:Mutex::new(vec![]),discovery:Mutex::new(None),identity:Mutex::new(Some(identity)),workers:Mutex::new(vec![])});
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
            let properties=[("session",id.as_str()),("label",label.as_str()),("platform",std::env::consts::OS),("version","1"),("mode",mode),("protocol",if session.file_intent.is_some(){auth::Protocol::Files.id()}else{auth::Protocol::Vault.id()}),("role",session.file_intent.as_ref().map(|i|i.role()).unwrap_or("vault")),("file_chunk",if session.file_intent.is_some(){"1048576"}else{"0"}),("file_entries",if session.file_intent.is_some(){"100000"}else{"0"})];
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
                                if !addresses.is_empty() && state.peers.len()<100 { state.peers.insert(info.get_fullname().to_string(),Peer{session:id.into(),label:info.get_property_val_str("label").unwrap_or("Tenjee Vault").chars().take(64).collect(),platform:info.get_property_val_str("platform").unwrap_or("unknown").chars().take(32).collect(),mode:info.get_property_val_str("mode").unwrap_or("vault").chars().take(16).collect(),protocol:info.get_property_val_str("protocol").unwrap_or("").chars().take(96).collect(),role:info.get_property_val_str("role").unwrap_or("").chars().take(16).collect(),addresses}); }
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
                        let Some(code)=owner.claim_pairing() else{continue;};
                        if owner.register_socket(&socket).is_err() { break; }
                        let identity=owner.identity.lock().unwrap().clone();
                        // Preserve authentication errors: a socket-mode or TLS
                        // failure must not be reduced to "check the code".
                        let authenticated=identity.ok_or_else(||VaultError::Validation("Temporary pairing identity is unavailable".into()))
                            .and_then(|identity|auth::server_for(if owner.file_intent.is_some(){auth::Protocol::Files}else{auth::Protocol::Vault},socket,&identity,&owner.id,&code));
                        match authenticated {
                            Ok(mut stream)=>{
                                if Instant::now()>=owner.state().expires || owner.check().is_err() { owner.cancel("Pairing code expired"); break; }
                                owner.authenticated();
                                let _=stream.sock.set_read_timeout(Some(Duration::from_secs(if owner.file_intent.is_some(){120}else{1800})));
                                if owner.file_intent.is_some(){let _=stream.sock.set_write_timeout(Some(Duration::from_secs(120)));}
                                let result=owner.exchange(&app,&mut stream,true);
                                // A successful exchange has already received the
                                // committed receipt. Shutdown errors cannot undo it.
                                let _=auth::close(&mut stream);
                                if let Err(error)=result { owner.phase("failed",&serde_json::to_string(&error.payload()).unwrap_or_else(|_|"Exchange failed".into())); }
                                owner.cancel("Exchange ended; pair again to resume");
                                break;
                            }
                            Err(error)=>{
                                if owner.check().is_err(){break;}
                                if owner.pairing_failed(&error){break;}
                            }
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
            let expired=owner.expired();
            if expired { owner.cancel("Exchange expired; generate a fresh pairing code"); break; }
            std::thread::sleep(Duration::from_millis(250));
        }
    }));
    Ok(session.status())
}

#[tauri::command]
pub async fn exchange_enter_cmd(app: tauri::AppHandle, label: String, interface: Option<String>, port: Option<u16>) -> VaultResult<Status> {
    enter(app,label,interface,port,None).await
}
pub async fn enter(app: tauri::AppHandle, label: String, interface: Option<String>, port: Option<u16>, file_intent: Option<crate::file_exchange::engine::Intent>) -> VaultResult<Status> {
    static ENTER:std::sync::OnceLock<tauri::async_runtime::Mutex<()>>=std::sync::OnceLock::new();
    let _enter=ENTER.get_or_init(||tauri::async_runtime::Mutex::new(())).lock().await;
    let owner=app.clone();
    tauri::async_runtime::spawn_blocking(move ||owner.state::<ExchangeManager>().stop()).await.map_err(|_|VaultError::Validation("Exchange cleanup interrupted".into()))?;
    let token=uuid::Uuid::new_v4().to_string();
    app.state::<ExchangeManager>().1.store(true,Ordering::SeqCst);
    #[cfg(target_os="android")]
    {
        let owner=app.clone();
        let channel=tauri::ipc::Channel::<serde_json::Value>::new(move |event| {
            let activity=match event{tauri::ipc::InvokeResponseBody::Json(body)=>serde_json::from_str::<serde_json::Value>(&body).is_ok_and(|v|v["activity"]==true),_=>false};
            if activity{if let Ok(session)=owner.state::<ExchangeManager>().current(){session.touch();}}
            else{owner.state::<ExchangeManager>().cancel("App suspended; fresh pairing is required");}Ok(())
        });
        if let Err(error)=crate::mobile::call(app.clone(),"beginExchange",serde_json::json!({"suspended":channel,"token":token,"discovery":!network::Network::select(interface.as_deref())?.discovery_interfaces().is_empty()})).await {
            app.state::<ExchangeManager>().1.store(false,Ordering::SeqCst);
            let _=crate::mobile::call(app.clone(),"endExchange",serde_json::json!({"token":token})).await;
            return Err(error);
        }
    }
    let owner=app.clone(); let session_token=token.clone();
    let result=tauri::async_runtime::spawn_blocking(move || start(owner,label,session_token,interface,port.unwrap_or(0),file_intent)).await.map_err(|_|VaultError::Validation("Cannot start exchange mode".into()))?;
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
            let mut stream=auth::client_for(if owner.file_intent.is_some(){auth::Protocol::Files}else{auth::Protocol::Vault},socket,&code)?;
            owner.check()?;
            owner.authenticated();
            stream.sock.set_read_timeout(Some(Duration::from_secs(if owner.file_intent.is_some(){120}else{1800})))?;
            if owner.file_intent.is_some(){stream.sock.set_write_timeout(Some(Duration::from_secs(120)))?;}
            let result=owner.exchange(&app,&mut stream,false);
            let _=auth::close(&mut stream);
            result
        })();
        if let Err(error)=result { owner.phase("failed",&serde_json::to_string(&error.payload()).unwrap_or_else(|_|"Exchange failed".into())); }
        owner.cancel("Exchange ended; pair again to resume");
    }));
    Ok(())
}

#[tauri::command]
pub fn exchange_available_cmd()->bool {true}

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

#[cfg(all(test, not(target_os = "android")))]
mod tests {
    use super::*;
    use crate::db::{layout, startup};

    #[test]
    fn device_exchange_is_available_in_all_build_profiles() {
        assert!(exchange_available_cmd());
    }

    fn session() -> Arc<Session> {
        Arc::new(Session {
            id: uuid::Uuid::new_v4().to_string(),
            network: network::Network::unbound(),
            file_intent:None,file_root:std::path::PathBuf::new(),
            inner: Mutex::new(Mutable {
                status: Status {
                    label: "Test device".into(),
                    ..Status::default()
                },
                code: Zeroizing::new(String::new()),
                expires: Instant::now() + Duration::from_secs(300),
                busy: false,
                approved: false,
                touched: Instant::now(),
                peers: BTreeMap::new(),
            }),
            cancelled: AtomicBool::new(false),
            accepting: AtomicBool::new(true),
            sockets: Mutex::new(Vec::new()),
            listeners: Mutex::new(Vec::new()),
            discovery: Mutex::new(None),
            identity: Mutex::new(None),
            workers: Mutex::new(Vec::new()),
        })
    }

    fn workspace() -> (tempfile::TempDir, AppState) {
        let directory = tempfile::tempdir().unwrap();
        let root = layout::data_root(directory.path());
        let report = startup::startup(&root).unwrap();
        let state = AppState::init(root, report).unwrap();
        (directory, state)
    }

    fn exchange_round(left: &AppState, right: &AppState) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let client_socket = TcpStream::connect(address).unwrap();
        let receiver = session();
        let connector = session();
        let receiving = receiver.clone();
        let connecting = connector.clone();
        let left_inner = left.inner.clone();
        let right_inner = right.inner.clone();
        let server = std::thread::spawn(move || {
            let (socket, _) = listener.accept().unwrap();
            // Match the nonblocking socket accepted by the Windows app.
            #[cfg(not(target_os = "windows"))]
            socket.set_nonblocking(true).unwrap();
            receiving.register_socket(&socket).unwrap();
            let mut stream = auth::server(
                socket,
                &auth::ServerIdentity::new().unwrap(),
                &receiving.id,
                "01234567",
            )
            .unwrap();
            stream
                .sock
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let result = receiving.exchange_workspace(&right_inner, &mut stream, true);
            auth::close(&mut stream).unwrap();
            receiving.cancel("Exchange ended; pair again to resume");
            result
        });
        let client = std::thread::spawn(move || {
            let socket = client_socket;
            connecting.register_socket(&socket).unwrap();
            let mut stream =
                auth::client(socket, "01234567").unwrap();
            stream
                .sock
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let result = connecting.exchange_workspace(&left_inner, &mut stream, false);
            auth::close(&mut stream).unwrap();
            connecting.cancel("Exchange ended; pair again to resume");
            result
        });
        let deadline = Instant::now() + Duration::from_secs(10);
        while receiver.status().phase != "approval" || connector.status().phase != "approval" {
            assert!(Instant::now() < deadline, "Peers did not reach approval");
            std::thread::sleep(Duration::from_millis(10));
        }
        receiver.state().approved = true;
        connector.state().approved = true;
        let left_result = client.join().unwrap();
        let right_result = server.join().unwrap();
        assert!(left_result.is_ok(), "Connector failed: {left_result:?}");
        assert!(right_result.is_ok(), "Receiver failed: {right_result:?}");
        assert_eq!(receiver.status().phase, "finished");
        assert_eq!(connector.status().phase, "finished");
        assert_eq!(receiver.status().result.unwrap().pending_groups, 0);
        assert_eq!(connector.status().result.unwrap().pending_groups, 0);
    }

    #[test]
    fn workspace_failure_reports_the_reason_to_the_other_device() {
        for server_fails in [true, false] {
            let (_left_dir, left) = workspace();
            let (_right_dir, right) = workspace();
            let broken = if server_fails { &right } else { &left };
            let attachment = broken.inner.with_tasks(|conn| {
                let list = crate::tasks::lists::create_list(conn, "Missing attachment", None)?;
                let task = crate::tasks::tasks::create_task(conn, &list.id, None, "Missing attachment")?;
                crate::tasks::attachments::save_attachment(&broken.inner.tasks_files_dir(), conn,
                    &task.id, "missing.bin", None, b"missing attachment")
            }).unwrap();
            std::fs::remove_file(crate::blob_store::blob_path(&broken.inner.tasks_files_dir(), &attachment.hash)).unwrap();

            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            let receiver = session();
            let connector = session();
            let server = std::thread::spawn(move || {
                let (socket, _) = listener.accept().unwrap();
                receiver.register_socket(&socket).unwrap();
                let mut stream = auth::server(socket, &auth::ServerIdentity::new().unwrap(),
                    &receiver.id, "01234567").unwrap();
                let result = receiver.exchange_workspace(&right.inner, &mut stream, true);
                let _ = auth::close(&mut stream);
                receiver.cancel("Exchange ended");
                result
            });
            let socket = TcpStream::connect(address).unwrap();
            connector.register_socket(&socket).unwrap();
            let mut stream = auth::client(socket, "01234567").unwrap();
            let client_result = connector.exchange_workspace(&left.inner, &mut stream, false);
            let _ = auth::close(&mut stream);
            connector.cancel("Exchange ended");
            let server_result = server.join().unwrap();
            let (local, remote) = if server_fails { (server_result, client_result) } else { (client_result, server_result) };
            let detail = local.unwrap_err().payload().params.into_iter().find(|(key, _)| *key == "detail").unwrap().1;
            assert!(matches!(&remote, Err(VaultError::Validation(message)) if message == &format!("Peer exchange failed: {detail}")), "server_fails={server_fails}, peer result: {remote:?}, local detail: {detail}");
        }
    }

    #[test]
    fn confirmed_exchange_transfers_attachments_and_repeats_without_duplicates() {
        let (_left_dir, left) = workspace();
        let (_right_dir, right) = workspace();
        // Larger than two transfer chunks, with distinct content in each direction.
        let left_bytes: Vec<u8> = (0..600_000).map(|n| (n % 251) as u8).collect();
        let right_bytes: Vec<u8> = (0..550_000).map(|n| (n % 239) as u8).collect();
        let left_attachment = left
            .inner
            .with_tasks(|conn| {
                let list = crate::tasks::lists::create_list(conn, "From left", None)?;
                let task =
                    crate::tasks::tasks::create_task(conn, &list.id, None, "Task from left")?;
                crate::tasks::attachments::save_attachment(
                    &left.inner.tasks_files_dir(),
                    conn,
                    &task.id,
                    "左附件.bin",
                    None,
                    &left_bytes,
                )
            })
            .unwrap();
        let right_attachment = right
            .inner
            .with_tasks(|conn| {
                let list = crate::tasks::lists::create_list(conn, "From right", None)?;
                let task =
                    crate::tasks::tasks::create_task(conn, &list.id, None, "Task from right")?;
                crate::tasks::attachments::save_attachment(
                    &right.inner.tasks_files_dir(),
                    conn,
                    &task.id,
                    "right.bin",
                    None,
                    &right_bytes,
                )
            })
            .unwrap();
        for _ in 0..2 {
            exchange_round(&left, &right);
            for state in [&left, &right] {
                assert_eq!(
                    crate::blob_store::read(&state.inner.tasks_files_dir(), &left_attachment.hash)
                        .unwrap(),
                    left_bytes
                );
                assert_eq!(
                    crate::blob_store::read(&state.inner.tasks_files_dir(), &right_attachment.hash)
                        .unwrap(),
                    right_bytes
                );
                state.inner.with_tasks(|conn| {
                    let count: i64 = conn.query_row(
                        "SELECT COUNT(*) FROM tasks WHERE title IN ('Task from left','Task from right')",
                        [], |row| row.get(0),
                    )?;
                    assert_eq!(count, 2);
                    let count: i64 = conn.query_row(
                        "SELECT COUNT(*) FROM attachments WHERE id IN (?1,?2)",
                        [&left_attachment.id, &right_attachment.id], |row| row.get(0),
                    )?;
                    assert_eq!(count, 2);
                    Ok(())
                }).unwrap();
            }
        }
    }
    #[test]
    fn pairing_attempts_are_serial_globally_bounded_expiring_and_single_use(){
        let owner=session();{let mut state=owner.state();state.code=Zeroizing::new("01234567".into());state.status.phase="discovering".into();state.status.attempts_left=5;}
        let barrier=Arc::new(std::sync::Barrier::new(16));let mut threads=Vec::new();
        for _ in 0..16 {let owner=owner.clone();let barrier=barrier.clone();threads.push(std::thread::spawn(move||{barrier.wait();owner.claim_pairing().is_some()}));}
        assert_eq!(threads.into_iter().filter_map(|t|t.join().ok()).filter(|v|*v).count(),1);
        let error=VaultError::Io(std::io::Error::from(std::io::ErrorKind::WouldBlock));
        for remaining in (0..5).rev() {
            assert_eq!(owner.pairing_failed(&error),remaining==0);
            assert_eq!(owner.status().attempts_left,remaining);
            if remaining>0 {
                assert_eq!(owner.status().message,serde_json::to_string(&error.payload()).unwrap());
                assert!(owner.claim_pairing().is_some());
            } else {
                assert_eq!(owner.status().message,"Pairing code invalidated after five failed attempts");
            }
        }
        assert!(owner.claim_pairing().is_none());assert!(owner.state().code.is_empty());assert!(owner.check().is_err());
        let owner=session();{let mut state=owner.state();state.code=Zeroizing::new("01234567".into());state.status.phase="discovering".into();state.status.attempts_left=5;state.expires=Instant::now()-Duration::from_secs(1);}
        assert!(owner.expired());assert!(owner.claim_pairing().is_none());owner.state().expires=Instant::now()+Duration::from_secs(300);assert!(owner.claim_pairing().is_some());owner.authenticated();assert!(owner.state().code.is_empty());assert!(owner.claim_pairing().is_none());
    }
    #[test]
    fn stop_closes_registered_sockets_and_clears_authorization(){
        let owner=session();owner.state().code=Zeroizing::new("01234567".into());let listener=TcpListener::bind("127.0.0.1:0").unwrap();let mut peer=TcpStream::connect(listener.local_addr().unwrap()).unwrap();let (socket,_)=listener.accept().unwrap();owner.register_socket(&socket).unwrap();owner.cancel("Stopped");
        use std::io::Read;peer.set_read_timeout(Some(Duration::from_secs(1))).unwrap();assert_eq!(peer.read(&mut [0;1]).unwrap(),0);assert!(owner.state().code.is_empty());assert!(owner.identity.lock().unwrap().is_none());assert!(owner.listeners.lock().unwrap().is_empty());assert!(owner.sockets.lock().unwrap().is_empty());
    }

    #[test]
    fn file_watchdog_distinguishes_waiting_approval_and_meaningful_progress(){
        let mut owner=session();Arc::get_mut(&mut owner).unwrap().file_intent=Some(crate::file_exchange::engine::Intent::Send{batch:uuid::Uuid::new_v4().to_string()});
        {let mut state=owner.state();state.status.phase="exchanging".into();state.touched=Instant::now()-Duration::from_secs(121);}
        assert!(owner.expired());owner.touch();assert!(!owner.expired());
        {let mut state=owner.state();state.status.phase="approval".into();state.touched=Instant::now()-Duration::from_secs(121);}
        assert!(!owner.expired());owner.state().touched=Instant::now()-Duration::from_secs(1801);assert!(owner.expired());
    }

}
